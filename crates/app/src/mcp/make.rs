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

use std::collections::BTreeSet;

use glam::DVec2;
use varde_document::{
    AxisLine, BodyId, Command, Document, FeatureId, FeatureKind, MAX_COORD, Named,
};
use varde_expr::{Ask, Value as ExprValue};
use varde_mcp::tools::{
    self, Axis, ExtrudeExtent, Naming, Operation, Profile, Profiles, RevolveExtent,
};
use varde_view::{Angle, Distance, ExtentKind, OperationKind, TurnKind};

use super::edit::{Made, Refused, body_named, feature_named};
use crate::doc::{
    BodyTargets, CombineSession, Doc, ExtrudeSession, Marks, Merges, RegionPick, RevolveSession,
    merged_in, pickable, sketch_of,
};

/// Which of a tool's arguments to take: all of them, for a new
/// feature (those not given at their defaults, as the panel starts), or
/// those a `set_feature` patch gives, on a session opened on the
/// feature, the rest kept as it has them.
#[derive(Clone, Copy)]
pub(super) enum Given<'a> {
    All,
    Only(&'a BTreeSet<String>),
}

impl Given<'_> {
    /// Whether the argument `key` is taken.
    pub(super) fn has(self, key: &str) -> bool {
        match self {
            Given::All => true,
            Given::Only(keys) => keys.contains(key),
        }
    }

    /// Whether the argument `key` is sent: for a new feature, if it's
    /// `present` (given, or a flag on); for a patch, if it gives it.
    pub(super) fn sent(self, key: &str, present: bool) -> bool {
        match self {
            Given::All => present,
            Given::Only(keys) => keys.contains(key),
        }
    }
}

/// Why an edited extrude's or revolve's Flip can't be set: its extent
/// ignores it (stored as it was).
const FLIP_EXTENT: &str = "flip goes with extent one_side or two_sides";

/// The tool a feature's refusals go under while it's edited.
pub(super) const SET_FEATURE: &str = "set_feature";

/// The name a tool's refusals go under: its own for a new feature, or
/// [`SET_FEATURE`] for the feature `edited`.
pub(super) fn tool_for(tool: &'static str, edited: Option<FeatureId>) -> &'static str {
    match edited {
        Some(_) => SET_FEATURE,
        None => tool,
    }
}

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

/// Takes `sketch` and `profiles`, those `given`, into `regions` as the
/// panel's clicks pick them, for the feature `edited` (or a new one):
/// a sketch other than the one it has needs its profiles, which replace
/// those picked (the edited feature's not found among them).
pub(super) fn regions(
    doc: &Doc,
    regions: &mut RegionPick,
    tool: &str,
    edited: Option<FeatureId>,
    sketch: &str,
    profiles: &Profiles,
    given: Given,
) -> Result<(), Refused> {
    let document = doc.editor.document();
    let wrong = |why: String| failed(tool, why);
    if edited.is_some() && given.has("sketch") {
        let sketch = sketch_named(document, tool, sketch, edited)?;
        if regions.source != Some(sketch) {
            if !given.has("profiles") {
                let why = "sketch goes with profiles: the regions of the new sketch";
                return Err(wrong(why.to_owned()));
            }
            *regions = RegionPick::new(Some(sketch), regions.most());
        }
    }
    if given.has("profiles") {
        let sketch = regions
            .source
            .ok_or_else(|| wrong(gone_sketch("feature")))?;
        while regions.missing > 0 {
            regions.drop_missing(0);
        }
        pick(regions, document, sketch, profiles).map_err(wrong)?;
    }
    Ok(())
}

/// Why a feature's regions can't be picked: its sketch is gone.
fn gone_sketch(feature: &str) -> String {
    format!("the {feature}'s sketch is gone: give sketch with profiles")
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

/// The bodies `names`, as clicks on them pick them for the `feature`
/// `edited`, or a new one (refusals the `tool`'s): a body merged into
/// another before it (before where a new one goes) as the body holding
/// it, each one made before it and named once: two names of one body (or
/// a body merged into another and that one) are said rather than picked
/// as a click on it again would take it out.
pub(super) fn picked_bodies(
    doc: &Doc,
    tool: &str,
    feature: &str,
    names: &[String],
    edited: Option<FeatureId>,
) -> Result<Vec<BodyId>, Refused> {
    let document = doc.editor.document();
    let merged = committed_merges(doc, edited).map_err(|why| failed(tool, why))?;
    let mut bodies = Vec::with_capacity(names.len());
    for name in names {
        let id = body_named(document, name)?;
        let id = merged.holder(id).unwrap_or(id);
        if !pickable(document, id, edited) {
            return Err(failed(tool, made_after(name, feature, edited)));
        }
        if bodies.contains(&id) {
            let name = varde_mcp::shown(name);
            let why = format!("\"{name}\" is named twice (or a body it was merged into is)");
            return Err(failed(tool, why));
        }
        bodies.push(id);
    }
    Ok(bodies)
}

/// What naming and merging read of `doc`'s committed model, which the
/// client's names and keys are resolved against (not the model shown,
/// which may be the preview of a panel the client's edit closed): why
/// not, worded for the client, before there's one or while it's being
/// rebuilt.
pub(super) fn committed_marks(doc: &Doc) -> Result<Marks<'_>, &'static str> {
    super::query::committed(doc)?;
    doc.feed.committed_marks().ok_or(super::query::REBUILDING)
}

/// The bodies merged into others before the feature `edited` (before
/// where a new one goes) in `doc`'s committed model ([`merged_in`] over
/// [`committed_marks`]), or why not.
pub(super) fn committed_merges(
    doc: &Doc,
    edited: Option<FeatureId>,
) -> Result<Merges, &'static str> {
    Ok(merged_in(
        doc.editor.document(),
        edited,
        committed_marks(doc)?,
    ))
}

/// Adding `kind` to `document` where its features go, and what that
/// makes: the feature, named as the panel's OK names it ([`named`]
/// gives it the client's names).
pub(super) fn added(document: &Document, kind: FeatureKind) -> (Vec<Command>, Made) {
    let command = document.add_feature(kind);
    let name = match &command {
        Command::AddFeature { name, .. } => name.clone(),
        _ => String::new(),
    };
    let made = Made::Added {
        name,
        notes: Vec::new(),
    };
    (vec![command], made)
}

/// `planned`, the commands a feature tool `tool` makes adding a feature
/// (the first, `Command::AddFeature`) and what it's to make, with the
/// names `naming` asks for: the feature's in the command, its new
/// bodies' by renames after it (in the one undo step), each checked and
/// numbered on a clash as `rename`'s ([`Document::new_name`],
/// [`Document::rename`]). Body names it makes no body for are refused,
/// as are more than it makes bodies; those it makes beyond the names
/// keep their defaults.
pub(super) fn named(
    doc: &Doc,
    tool: &str,
    naming: &Naming,
    planned: (Vec<Command>, Made),
) -> Result<(Vec<Command>, Made), Refused> {
    let (mut commands, made) = planned;
    let (Some(Command::AddFeature { name, .. }), Made::Added { notes, .. }) =
        (commands.first_mut(), &made)
    else {
        return Ok((commands, made));
    };
    let mut notes = notes.clone();
    if let Some(wanted) = &naming.name {
        *name = given_name(doc.editor.document(), tool, wanted, &mut notes)?;
    }
    let name = name.clone();
    if !naming.bodies.is_empty() {
        commands.extend(body_names(
            doc,
            tool,
            &commands[0],
            &naming.bodies,
            &mut notes,
        )?);
    }
    Ok((commands, Made::Added { name, notes }))
}

/// The name a new feature or sketch made by `tool` asking for `wanted`
/// gets on `document` ([`Document::new_name`]), with a note in `notes`
/// if it was taken; refused if it's empty.
pub(super) fn given_name(
    document: &Document,
    tool: &str,
    wanted: &str,
    notes: &mut Vec<String>,
) -> Result<String, Refused> {
    let given = document
        .new_name(wanted)
        .ok_or_else(|| failed(tool, "name is empty"))?;
    if let Some(taken) = &given.taken {
        notes.push(super::edit::taken_note(taken, &given.name));
    }
    Ok(given.name)
}

/// The renames giving the bodies `add`, a `Command::AddFeature`, makes
/// the names `wanted`, in the order the result lists them (the
/// document's), with a note for each name taken. Worked out on a copy
/// of the editor with `add` applied, as the client's apply will apply
/// it: the ids it gives are the same. One the editor refuses is left to
/// that apply to refuse.
fn body_names(
    doc: &Doc,
    tool: &str,
    add: &Command,
    wanted: &[String],
    notes: &mut Vec<String>,
) -> Result<Vec<Command>, Refused> {
    let before = doc.editor.document();
    let mut editor = doc.editor.clone();
    if editor.apply(add.clone()).is_err() {
        return Ok(Vec::new());
    }
    let made: Vec<BodyId> = (editor.document().bodies().iter())
        .filter(|body| before.body(body.id).is_none())
        .map(|body| body.id)
        .collect();
    if made.is_empty() {
        let why = match tool {
            "split" => "body_name: this split makes no new body: only keep both makes one",
            "pattern" => {
                "body_names: this pattern makes no new body: only join false makes its copies bodies"
            }
            _ => "body_name: this makes no new body: only operation new_body makes one",
        };
        return Err(failed(tool, why));
    }
    if wanted.len() > made.len() {
        let why = format!(
            "body_names: {} names for the {} bodies it makes",
            wanted.len(),
            made.len()
        );
        return Err(failed(tool, why));
    }
    let mut commands = Vec::with_capacity(wanted.len());
    for (&id, wanted) in made.iter().zip(wanted) {
        let document = editor.document();
        let Some(rename) = document.rename(Named::Body(id), wanted) else {
            if wanted.trim().is_empty() {
                return Err(failed(tool, "a body's name is empty"));
            }
            continue;
        };
        if let (Some(taken), Command::Rename { name, .. }) = (&rename.taken, &rename.command) {
            notes.push(super::edit::taken_note(taken, name));
        }
        if editor.apply(rename.command.clone()).is_err() {
            return Ok(commands);
        }
        commands.push(rename.command);
    }
    Ok(commands)
}

/// The arguments' refusal for `tool`: `why`, the row failed.
pub(super) fn failed(tool: &str, why: impl std::fmt::Display) -> Refused {
    Refused::failed(format!("{tool}: {why}"))
}

/// Why the body `name` can't be named: it's made after where the
/// `feature` goes.
/// `feature` the one `edited`, if it is one.
fn made_after(name: &str, feature: &str, edited: Option<FeatureId>) -> String {
    let name = varde_mcp::shown(name);
    match edited {
        None => format!("\"{name}\" is made after where the {feature} goes (the rollback marker)"),
        Some(_) => {
            format!("\"{name}\" is made after the {feature}, which can only name what's before it")
        }
    }
}

/// The sketch feature named `name`, made before the feature `edited` (or
/// where a new one goes), the `tool`'s.
pub(super) fn sketch_named(
    document: &Document,
    tool: &str,
    name: &str,
    edited: Option<FeatureId>,
) -> Result<FeatureId, Refused> {
    let id = feature_named(document, name)?;
    if sketch_of(document, id).is_none() {
        let why = format!("\"{}\" isn't a sketch", varde_mcp::shown(name));
        return Err(Refused::failed(why));
    }
    let at = match edited {
        None => Some(document.insert_at()),
        Some(edited) => document.feature_index(edited),
    };
    let before = (document.feature_index(id))
        .zip(at)
        .is_some_and(|(id, at)| id < at);
    if !before {
        let feature = if edited.is_some() { "feature" } else { tool };
        return Err(failed(tool, made_after(name, feature, edited)));
    }
    Ok(id)
}

/// A field's reading `error`, if any, naming the argument `said`
/// typed into it.
pub(super) fn read(error: Option<&impl std::fmt::Display>, said: &str) -> Result<(), String> {
    match error {
        Some(error) => Err(format!("{said}: {error}")),
        None => Ok(()),
    }
}

/// The panel's operation for the tools'.
pub(super) fn operation(operation: Operation) -> OperationKind {
    match operation {
        Operation::NewBody => OperationKind::NewBody,
        Operation::Join => OperationKind::Join,
        Operation::Cut => OperationKind::Cut,
        Operation::Intersect => OperationKind::Intersect,
    }
}

/// The bodies `names` a join, cut or intersect leaves out, as the
/// panel's Bodies list unticks them, for the feature `edited` or a new
/// one, `new_body` whether it makes one (which has no such list): each
/// one made before it. Refusals are `tool`'s.
pub(super) fn excluded(
    doc: &Doc,
    tool: &str,
    names: &[String],
    new_body: bool,
    edited: Option<FeatureId>,
) -> Result<BodyTargets, Refused> {
    let document = doc.editor.document();
    if new_body && !names.is_empty() {
        let why = "exclude goes with operation join, cut or intersect".to_owned();
        return Err(failed(tool, why));
    }
    // A merged body is excluded as the body holding it, which is what
    // the model has and the panel's clicks pick.
    let merged = committed_merges(doc, edited).map_err(|why| failed(tool, why))?;
    let mut excluded = BTreeSet::new();
    for name in names {
        let body = body_named(document, name)?;
        let body = merged.holder(body).unwrap_or(body);
        if !pickable(document, body, edited) {
            return Err(failed(tool, made_after(name, "feature", edited)));
        }
        excluded.insert(body);
    }
    Ok(BodyTargets::new(&Vec::from_iter(excluded)))
}

/// Picks `profiles` of `sketch` in `regions`, as the panel's clicks
/// pick them: why not, if the sketch's regions can't be found, an index
/// is past them, a point is in none, or a region is too thin to be
/// referenced.
pub(super) fn pick(
    regions: &mut RegionPick,
    document: &Document,
    sketch: FeatureId,
    profiles: &Profiles,
) -> Result<(), String> {
    regions.refresh(document);
    let found = regions
        .found(sketch)
        .ok_or("the sketch is too complex for its regions to be found")?;
    let all = &found.profiles;
    let count = all.regions.len();
    if count == 0 {
        return Err("the sketch has no closed regions".to_owned());
    }
    let picked: BTreeSet<usize> = match profiles {
        Profiles::All => (0..count).collect(),
        Profiles::Picked(items) => {
            let design = document.design();
            let ask = Ask::length(design.units, f64::from(MAX_COORD)).with_params(design.params);
            let mut picked = BTreeSet::new();
            for item in items {
                let index = match item {
                    Profile::Index(index) if *index < count => *index,
                    Profile::Index(index) => {
                        return Err(format!(
                            "the sketch has {count} region{}: index {index} is past them",
                            if count == 1 { "" } else { "s" }
                        ));
                    }
                    Profile::Point([x, y]) => {
                        let read = |text: &str, said: &str| {
                            ExprValue::new(text.trim(), &ask)
                                .map(|value| value.value)
                                .map_err(|why| format!("the point's {said}: {why}"))
                        };
                        let at = DVec2::new(read(x, "x")?, read(y, "y")?);
                        all.region_at(at).ok_or_else(|| {
                            format!("no region of the sketch has the point ({x}, {y}) inside")
                        })?
                    }
                };
                picked.insert(index);
            }
            picked
        }
    };
    if let Some(&thin) = picked.iter().find(|&&index| all.reference(index).is_none()) {
        return Err(format!(
            "region {thin} is too thin to be referenced: leave it out"
        ));
    }
    let wanted = picked.len();
    regions.pick(picked);
    if regions.picked.len() != wanted {
        return Err("too many regions for one feature".to_owned());
    }
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
            let drawn = sketch_of(document, sketch).ok_or("the sketch is gone")?;
            let curve = (drawn.curves.iter())
                .find(|curve| curve.id.get() == *number)
                .ok_or_else(|| format!("the sketch has no curve {number}"))?;
            if !matches!(curve.curve, varde_sketch::Curve::Line { .. }) {
                return Err(format!("curve {number} of the sketch isn't a line"));
            }
            AxisLine::Curve(curve.id)
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

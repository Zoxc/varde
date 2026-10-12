//! The argument vocabulary the MCP client's feature tools share (see
//! `make`, `motion`, `modify`, `blend` and `sweep`): which arguments a
//! call or `set_feature` patch gives ([`Given`]), their refusals
//! ([`failed`], [`Wrong`], [`said`], [`goes_with`]), reading typed text
//! and keys into a session ([`typed`], [`by_key`], [`keyed`]), bodies,
//! sketches, curves, points, regions, faces and edges named as the
//! panels' clicks pick them, and adding the feature a session sets up
//! ([`added`], [`named`], [`made`], [`made_of`]).

use std::borrow::Cow;
use std::collections::BTreeSet;

use glam::DVec2;
use varde_document::{
    Axis3, BodyId, Command, Document, FeatureId, FeatureKind, MAX_COORD, Named, OriginPlane,
};
use varde_expr::{Ask, Value as ExprValue};
use varde_mcp::tools::{
    Item, ItemKind, Naming, Operation, Profile, Profiles, WorldAxis, WorldPlane,
};
use varde_view::{MotionField, MotionKind, OperationKind, Pick, PickIndex};

use super::edit::{Made, Refused, body_named, feature_named};
use super::key;
use crate::doc::{
    BodyTargets, Doc, Marks, Merges, MotionSession, RegionPick, merged_in, pickable, sketch_of,
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

    /// `given` if the argument `key` is taken, else `kept`.
    pub(super) fn or<T>(self, key: &str, given: T, kept: T) -> T {
        match self.has(key) {
            true => given,
            false => kept,
        }
    }

    /// Sets `slot` to `value` if the argument `key` is taken.
    pub(super) fn set<T>(self, key: &str, slot: &mut T, value: T) {
        if self.has(key) {
            *slot = value;
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
pub(super) fn gone_sketch(feature: &str) -> String {
    format!("the {feature}'s sketch is gone: give sketch with profiles")
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

/// [`typed`] if `given` has the argument `said`.
pub(super) fn typed_given(
    doc: &Doc,
    session: &mut MotionSession,
    given: Given,
    field: MotionField,
    text: &Option<String>,
    said: &str,
) -> Result<(), Wrong> {
    if !given.has(said) {
        return Ok(());
    }
    typed(doc, session, field, text, said)
}

/// Why a motion-kind tool refuses its arguments: in words, which
/// [`made`] says as the tool's ([`failed`]), or refused whole already.
pub(super) enum Wrong {
    Said(String),
    Refused(Refused),
}

impl Wrong {
    /// It as the refusal of `tool`.
    pub(super) fn of(self, tool: &str) -> Refused {
        match self {
            Wrong::Said(why) => failed(tool, why),
            Wrong::Refused(refused) => refused,
        }
    }
}

impl From<String> for Wrong {
    fn from(why: String) -> Self {
        Wrong::Said(why)
    }
}

impl From<Refused> for Wrong {
    fn from(refused: Refused) -> Self {
        Wrong::Refused(refused)
    }
}

/// The commands a motion-kind tool, `tool`, makes on `doc`'s design,
/// and what it's to make: a session setting up `kind` of the bodies
/// `names` (picked as clicks on them pick them, [`picked_bodies`]), set
/// up by `set`, then added where the panel's OK adds it ([`made_of`]).
pub(super) fn made(
    doc: &Doc,
    tool: &str,
    kind: MotionKind,
    names: &[String],
    set: impl FnOnce(&mut MotionSession) -> Result<(), Wrong>,
) -> Result<(Vec<Command>, Made), Refused> {
    let bodies = picked_bodies(doc, tool, &kind.noun().to_lowercase(), names, None)?;
    made_of(doc, tool, kind, bodies, set)
}

/// The commands `tool` makes on `doc`'s design, and what it's to make:
/// a session setting up `kind` of `bodies` for the client
/// ([`MotionSession::for_client`]), set up by `set`, the feature it sets
/// up then added where the panel's OK adds it; refused, saying why in
/// the panel's words, where its OK would be off (a sweep's or loft's
/// names the document refuses at its place among those,
/// [`Doc::motion_held_in`]).
pub(super) fn made_of(
    doc: &Doc,
    tool: &str,
    kind: MotionKind,
    bodies: Vec<BodyId>,
    set: impl FnOnce(&mut MotionSession) -> Result<(), Wrong>,
) -> Result<(Vec<Command>, Made), Refused> {
    let document = doc.editor.document();
    let mut session = MotionSession::for_client(kind, document, bodies);
    set(&mut session).map_err(|wrong| wrong.of(tool))?;
    let kind = (session.built(&document.design())).map_err(|why| failed(tool, why))?;
    if let Some(why) = doc.motion_held_in(&session) {
        return Err(failed(tool, why));
    }
    Ok(added(document, kind))
}

/// Types `text`, if given, into `field` of `session`, trimmed, as typed
/// into the panel's: why not, naming the argument `said`, if the field
/// refuses it.
pub(super) fn typed(
    doc: &Doc,
    session: &mut MotionSession,
    field: MotionField,
    text: &Option<String>,
    said: &str,
) -> Result<(), Wrong> {
    let Some(text) = text else {
        return Ok(());
    };
    session.input(field, text.trim().to_owned(), doc.editor.document());
    Ok(read(session.field_error(field), said)?)
}

/// The face, edge or corner (`kind`) the key `text` (the argument
/// `said`) names, found in the committed model ([`key::found`]) and
/// taken by `take` as a click on it there is, with what that gave: why
/// not, naming the argument, if the key or `take` refuses it.
pub(super) fn by_key<T>(
    doc: &Doc,
    text: &str,
    kind: ItemKind,
    said: &str,
    take: impl FnOnce(&PickIndex, Pick) -> Result<T, Cow<'static, str>>,
) -> Result<(key::Found, T), String> {
    let found = key::found(doc, text, kind).map_err(|why| self::said(said, why))?;
    let taken = take(&found.index, found.pick).map_err(|why| self::said(said, why))?;
    Ok((found, taken))
}

/// The origin plane for the tools'.
pub(super) fn origin_plane(plane: WorldPlane) -> OriginPlane {
    match plane {
        WorldPlane::Xy => OriginPlane::XY,
        WorldPlane::Xz => OriginPlane::XZ,
        WorldPlane::Yz => OriginPlane::YZ,
    }
}

/// The world's axis for the tools'.
pub(super) fn world_axis(axis: WorldAxis) -> Axis3 {
    match axis {
        WorldAxis::X => Axis3::X,
        WorldAxis::Y => Axis3::Y,
        WorldAxis::Z => Axis3::Z,
    }
}

/// Takes the face or edge `item` names as `session`'s axis or plane
/// (the argument `said`), as a click on it in the committed model is
/// taken ([`Doc::reference_in`]).
pub(super) fn keyed(
    doc: &Doc,
    session: &mut MotionSession,
    item: &Item,
    said: &str,
) -> Result<(), String> {
    let (_, reference) = by_key(doc, &item.key, item.kind, said, |index, pick| {
        doc.reference_in(session, index, pick)
    })?;
    session.take_reference(reference);
    Ok(())
}

/// `why`, of the argument `said`.
pub(super) fn said(said: &str, why: impl Into<String>) -> String {
    format!("{said}: {}", why.into())
}

/// The refusal of the argument `name` given without what it `goes`
/// with.
pub(super) fn goes_with(name: &str, goes: &str) -> Wrong {
    Wrong::Said(format!("{name} goes with {goes}"))
}

/// Picks the faces `keys` in `session` if `given` has them: an edited
/// one's own taken out first, as clicks on them again would.
pub(super) fn faces_given(
    doc: &Doc,
    session: &mut MotionSession,
    keys: &[String],
    given: Given,
) -> Result<(), String> {
    if !given.has("faces") {
        return Ok(());
    }
    if session.feature.is_some() {
        session.faces = Default::default();
        session.refs_body();
    }
    faces(doc, session, keys)
}

/// Picks the faces `keys` in the face session `session`, each as a click
/// on it in the committed model picks it ([`Doc::pick_face`]): why not,
/// naming the argument, for a key refused, a face the panel refuses
/// (another body's, made later, past the limit), or one named twice
/// (two keys of one face too), which a click again would take out.
pub(super) fn faces(doc: &Doc, session: &mut MotionSession, keys: &[String]) -> Result<(), String> {
    for (at, text) in keys.iter().enumerate() {
        let argument = format!("faces[{at}]");
        let before = session.faces.refs.len();
        by_key(doc, text, ItemKind::Face, &argument, |index, pick| {
            doc.pick_face(session, index, pick)
        })?;
        if session.faces.refs.len() == before {
            return Err(said(&argument, "that face is named twice"));
        }
    }
    Ok(())
}

/// The curve numbered `number` of the sketch `sketch` of `document`, as
/// a click on it names it: why not, if the sketch hasn't one.
pub(super) fn curve_in(
    document: &Document,
    sketch: FeatureId,
    number: u32,
) -> Result<varde_sketch::Id, String> {
    let drawn = sketch_of(document, sketch).ok_or("the sketch is gone")?;
    (drawn.curves.iter())
        .find(|curve| curve.id.get() == number)
        .map(|curve| curve.id)
        .ok_or_else(|| format!("the sketch has no curve {number}"))
}

/// Picks the edges `keys` (the argument `argument`) in `session`, a
/// blend's or a sweep's path's, each as a click on it in the committed
/// model picks it ([`Doc::pick_edge`]): why not, naming the argument,
/// for a key refused, an edge the panel refuses (another body's, made
/// later, past the limit), or one a click would take out again (named
/// twice, or in the tangent chain of one named).
pub(super) fn edges(
    doc: &Doc,
    session: &mut MotionSession,
    keys: &[String],
    argument: &str,
) -> Result<(), String> {
    for (at, text) in keys.iter().enumerate() {
        let argument = format!("{argument}[{at}]");
        let (_, taken) = by_key(doc, text, ItemKind::Edge, &argument, |index, pick| {
            doc.pick_edge(session, index, pick)
        })?;
        if !taken {
            let why = "that edge is named twice, or is in the tangent chain of one named";
            return Err(said(&argument, why));
        }
    }
    Ok(())
}

/// The point numbered `number` of the sketch `sketch` of `document`, as
/// a click on it names it: why not, if the sketch hasn't one.
pub(super) fn point_in(
    document: &Document,
    sketch: FeatureId,
    number: u32,
) -> Result<varde_sketch::Id, String> {
    let drawn = sketch_of(document, sketch).ok_or("the sketch is gone")?;
    (drawn.points.iter())
        .find(|point| point.id.get() == number)
        .map(|point| point.id)
        .ok_or_else(|| format!("the sketch has no point {number}"))
}

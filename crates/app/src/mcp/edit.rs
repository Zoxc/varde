//! The MCP client's edits (see `notes/AgentSupport.md`, "Edits"): each the
//! `Command`s a panel's OK would make, applied through
//! [`Doc::apply_client`] (`Doc::apply`'s sibling: its refusal comes back
//! to the call, never to the user's `edit_error`), as one undo step.
//!
//! A read-only design refuses an edit, saying so; one that changes
//! nothing answers so at once. While the user's sketch edits wait on the
//! solver, it queues behind them (`Pending::Client`), so the undo order
//! is the order things happened; it's taken up in its turn only while
//! its call still waits (at [`BOUND`](super::waiting::BOUND) it's taken
//! out of the queue and answered not applied). Its names and keys are
//! resolved only once the model has caught up with the design
//! (`Wait::Model`), behind the client's calls before it. Once applied, it waits
//! for the first regeneration answer at its generation or past it, and
//! its result says what it made and what regeneration made of it.
//!
//! Removals go without the user's cascade or the delete prompt: one
//! that later features depend on is refused, naming them.
//!
//! New features (`extrude`, `revolve`, `combine`, made in `make.rs`) go
//! where the user's would: at the rollback marker (`Command::AddFeature`
//! puts them there).

use serde_json::{Value, json};
use varde_document::{
    BodyId, Command, Document, FeatureId, Generation, Named, Opacity, Removable, Revision, Tint,
    Tolerance,
};
use varde_expr::{Ask, LengthUnit, Value as ExprValue};
use varde_mcp::tools::{Call, Rename, Target, ToolReply, Units};
use varde_sketch::Id;
use varde_view::{McpCallState, ParamField};

use super::query;
use super::waiting::{From, Wait, Waiter};
use crate::doc::{Doc, params};

/// The answer to an edit that would change nothing.
pub(crate) const NOTHING_TO_CHANGE: &str = "Nothing to change: the design already is so";

/// What an edit's result says once applied and regenerated.
const APPLIED: &str = "applied";

/// What an edit's result says when the user took it back (an Undo, a
/// Revert) before regeneration answered.
pub(crate) const UNDONE: &str = "applied, then undone by the user";

/// The most names a result's listings give (failures, features not
/// built); the rest are counted.
const LISTED: usize = 50;

/// `made`, a result, with `notes` (the names asked for that were taken)
/// as its `note`, if there are any.
fn noted(made: &mut Value, notes: &[String]) {
    if !notes.is_empty() && made.is_object() {
        made["note"] = json!(notes.join("; "));
    }
}

/// What a name asked for that was `taken` got instead, `got`: a result's
/// note, as `rename`'s says it.
pub(super) fn taken_note(taken: &str, got: &str) -> String {
    format!(
        "The name \"{}\" was taken: it got \"{got}\"",
        varde_mcp::shown(taken)
    )
}

/// What an edit is to make, for its result once applied.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Made {
    /// `target` renamed; `taken` the name asked for, if another had it.
    Renamed {
        target: Named,
        taken: Option<String>,
    },
    /// A body shown or hidden, coloured, or made fainter: its result says
    /// how it's drawn now.
    Body(BodyId),
    /// Features removed with the bodies they made, by name, in order.
    Removed {
        features: Vec<String>,
        bodies: Vec<String>,
    },
    /// The parameter at this index added, set or renamed.
    Parameter(usize),
    /// The parameter so named removed.
    ParameterRemoved(String),
    /// The design's units or tolerance changed, or the rollback marker
    /// moved: its result says them as they are.
    Design,
    /// The feature `name` added: its result names it, its id and the
    /// bodies it makes, with `notes`: the names asked for that were
    /// taken.
    Added { name: String, notes: Vec<String> },
    /// The feature set by `set_feature`: its result as an added one's.
    Set(FeatureId),
    /// The sketch `name` added: its result names it and its id, with
    /// `notes` as an added feature's.
    NewSketch { name: String, notes: Vec<String> },
    /// The sketch put on another plane: its result names it, its id and
    /// its plane.
    Sketch(FeatureId),
    /// The link `link` added to the sketch `feature`: its result names
    /// the sketch, the link and the points and curves it made, as
    /// `sketch` lists them (they come with regeneration's relinking, so
    /// it's made again then).
    Link { feature: FeatureId, link: Id },
    /// The sketch `feature` edited by `edit_sketch`: its result names
    /// the sketch, maps each label to the ids it got (`labels`, made as
    /// the edit was planned), and gives the degrees of freedom left and
    /// the operations whose dimensions were placed as references, from
    /// the solver's answer.
    Drawn {
        feature: FeatureId,
        labels: Value,
        freedom: Option<usize>,
        referenced: Vec<usize>,
    },
}

impl Made {
    /// What the edit made, applied to `doc`'s design: its result's data.
    fn result(&self, doc: &Doc) -> Value {
        let document = doc.editor.document();
        match self {
            Made::Renamed { target, taken } => {
                let name = document.name_of(*target).unwrap_or_default();
                let mut made = match target {
                    Named::Feature(id) => json!({ "feature": name, "id": id.get() }),
                    Named::Body(_) => json!({ "body": name }),
                };
                if let Some(taken) = taken {
                    made["note"] = json!(taken_note(taken, name));
                }
                made
            }
            Made::Body(id) => match document.body(*id) {
                Some(body) => json!({
                    "body": body.name,
                    "visible": body.visible,
                    "color": body.color.map(|tint| json!({
                        "hue": tint.hue(),
                        "saturation": f64::from(tint.saturation()) / 100.0,
                    })),
                    "opacity": f64::from(body.opacity.percent()) / 100.0,
                }),
                None => json!({}),
            },
            Made::Removed { features, bodies } => json!({
                "removed": {
                    "features": listed(features.iter().map(|name| json!(name)).collect()),
                    "bodies": listed(bodies.iter().map(|name| json!(name)).collect()),
                }
            }),
            Made::Parameter(index) => {
                let mut made = json!({});
                if let Some(Value::Object(row)) = query::parameter(document, *index) {
                    for (key, value) in row {
                        // Its name as `parameter`, as the others name theirs.
                        let key = if key == "name" {
                            "parameter".to_owned()
                        } else {
                            key
                        };
                        made[key] = value;
                    }
                }
                made
            }
            Made::ParameterRemoved(name) => json!({ "removed": { "parameters": [name] } }),
            Made::Added { .. } | Made::Set(_) => {
                let feature = match self {
                    Made::Set(id) => document.feature(*id),
                    Made::Added { name, .. } => {
                        (document.features().iter()).find(|f| f.name == *name)
                    }
                    _ => None,
                };
                let mut made = match feature {
                    None => match self {
                        Made::Added { name, .. } => json!({ "feature": name }),
                        _ => json!({}),
                    },
                    Some(feature) => {
                        let bodies: Vec<Value> = (document.bodies().iter())
                            .filter(|body| body.created_by == feature.id)
                            .map(|body| json!(body.name))
                            .collect();
                        json!({
                            "feature": feature.name,
                            "id": feature.id.get(),
                            "bodies": listed(bodies),
                        })
                    }
                };
                if let Made::Added { notes, .. } = self {
                    noted(&mut made, notes);
                }
                made
            }
            Made::NewSketch { name, notes } => {
                let mut made =
                    match (document.features().iter()).find(|feature| feature.name == *name) {
                        Some(feature) => super::sketches::sketch_made(doc, feature.id),
                        None => json!({ "sketch": name }),
                    };
                noted(&mut made, notes);
                made
            }
            Made::Sketch(id) => super::sketches::sketch_made(doc, *id),
            Made::Link { feature, link } => super::sketches::link_made(doc, *feature, *link),
            Made::Drawn {
                feature,
                labels,
                freedom,
                referenced,
            } => super::draw::drawn_made(doc, *feature, labels, *freedom, referenced),
            Made::Design => {
                let rollback = (document.rollback())
                    .and_then(|id| document.feature(id))
                    .map(|feature| feature.name.clone());
                json!({
                    "units": document.units().symbol(),
                    "tolerance": varde_view::tolerance_label(document.tolerance().fit()),
                    "rollback": rollback,
                })
            }
        }
    }
}

/// Why an edit isn't applied: refused, the design unchanged, or failed
/// (a name not found, arguments not taken). Kept apart from its reply so
/// the edits of its session it fails can give the bare reason
/// ([`Waiting::not_applied`](super::waiting::Waiting::not_applied)).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Refused {
    /// Refused, `why` bare ("it's used by \"Extrude 1\""): answered
    /// [`not_applied`], its row not applied.
    NotApplied(String),
    /// Failed: answered this, its row failed.
    Failed(ToolReply),
}

impl Refused {
    /// Refused, `why`, the design unchanged.
    pub(crate) fn not_applied(why: impl std::fmt::Display) -> Self {
        Refused::NotApplied(why.to_string())
    }

    /// Failed, the error `why`.
    pub(crate) fn failed(why: impl Into<String>) -> Self {
        Refused::Failed(ToolReply::error(why))
    }

    /// Its reason, without what frames it: the edits of its session it
    /// fails say it.
    pub(crate) fn reason(&self) -> &str {
        match self {
            Refused::NotApplied(why) => why,
            Refused::Failed(reply) => &reply.text,
        }
    }

    /// The call's answer and what its row says.
    pub(crate) fn answer(self) -> (ToolReply, McpCallState) {
        match self {
            Refused::NotApplied(why) => (not_applied(why), McpCallState::NotApplied),
            Refused::Failed(reply) => (reply, McpCallState::Failed),
        }
    }
}

/// What an edit makes: the commands to apply and what they make, or a
/// sketch edit to propose to the solve lane first (`add_link`,
/// `edit_sketch`).
pub(super) enum Plan {
    Apply(Vec<Command>, Made),
    Propose(super::sketches::Proposed),
    /// `undo`: the session's own newest steps taken back (`undo.rs`),
    /// checked again as it applies, once what the user had open closed.
    Undo,
}

impl Plan {
    /// Whether it changes nothing: no commands.
    fn is_empty(&self) -> bool {
        matches!(self, Plan::Apply(commands, _) if commands.is_empty())
    }
}

/// What `call` makes on `doc`'s design ([`Plan`]), or why it can't be
/// made, as [`commands`]. `queued`: whether it's checked as it comes,
/// to queue behind the sketch edits waiting on the solver, a sketch
/// edit then checked on its sketch with theirs applied.
fn plan(doc: &Doc, call: &Call, from: &From, queued: bool) -> Result<Plan, Refused> {
    match call {
        Call::Undo { steps } => super::undo::plan(doc, *steps, from).map(|_| Plan::Undo),
        Call::AddLink(args) => super::sketches::add_link(doc, args).map(Plan::Propose),
        Call::EditSketch(args) => super::draw::edit_sketch(doc, args, queued).map(Plan::Propose),
        _ => commands(doc, call).map(|(commands, made)| Plan::Apply(commands, made)),
    }
}

/// The commands `call` makes on `doc`'s design, and what it's to make,
/// or why it can't be made: a name not found, arguments the design's
/// forms don't take, or a refusal the UI would make before the editor
/// (a removal others depend on, a parameter in use).
fn commands(doc: &Doc, call: &Call) -> Result<(Vec<Command>, Made), Refused> {
    let planned = made_commands(doc, call)?;
    match call.naming() {
        Some(naming) => super::args::named(doc, call.tool(), naming, planned),
        None => Ok(planned),
    }
}

/// The commands `call` makes, as [`commands`], what it makes named by
/// default.
fn made_commands(doc: &Doc, call: &Call) -> Result<(Vec<Command>, Made), Refused> {
    let document = doc.editor.document();
    let one = |command: Command, made: Made| Ok((vec![command], made));
    match call {
        Call::Rename(Rename { target, to }) => {
            let target = match target {
                Target::Feature(name) => Named::Feature(feature_named(document, name)?),
                Target::Body(name) => Named::Body(body_named(document, name)?),
            };
            // As the rename field commits it; none for the name it has:
            // nothing to change.
            let (commands, taken) = match document.rename(target, to) {
                Some(rename) => (vec![rename.command], rename.taken),
                None => (vec![], None),
            };
            Ok((commands, Made::Renamed { target, taken }))
        }
        // As the Objects panel makes them; one as it is changes nothing,
        // so the editor adds no step and the call says so.
        Call::SetVisible { body, visible } => {
            let id = body_named(document, body)?;
            one(Command::SetVisible(id, *visible), Made::Body(id))
        }
        Call::SetColor { body, color } => {
            let id = body_named(document, body)?;
            // In range (the tools' check), so as the sliders round it.
            let tint = color
                .map(|color| Tint::clamped(color.hue as f32, (color.saturation * 100.0) as f32));
            one(Command::SetColor(id, tint), Made::Body(id))
        }
        Call::SetOpacity { body, opacity } => {
            let id = body_named(document, body)?;
            let opacity = Opacity::clamped((opacity * 100.0) as f32);
            one(Command::SetOpacity(id, opacity), Made::Body(id))
        }
        Call::RemoveFeature { feature } => {
            let id = feature_named(document, feature)?;
            removal(document, Removable::Feature(id))
        }
        Call::RemoveBody { body } => {
            let id = body_named(document, body)?;
            removal(document, Removable::Body(id))
        }
        Call::AddParameter { name, value } => {
            let index = document.params().len();
            let command = Command::AddParam {
                name: name.trim().to_owned(),
                text: value.trim().to_owned(),
            };
            one(command, Made::Parameter(index))
        }
        // As the popup commits a field: none for what it is already.
        Call::SetParameter { name, value: text } | Call::RenameParameter { name, to: text } => {
            let index = param_named(document, name)?;
            let field = match call {
                Call::SetParameter { .. } => ParamField::Expression,
                _ => ParamField::Name,
            };
            let commands = params::param_command(document, index, field, text);
            Ok((commands.into_iter().collect(), Made::Parameter(index)))
        }
        Call::RemoveParameter { name } => {
            let index = param_named(document, name)?;
            // As the popup refuses it, before the editor would.
            let uses = params::uses(document, &document.params()[index].name);
            if !uses.is_empty() {
                let why = format!("it's used by {}: change those first", names(&uses));
                return Err(Refused::NotApplied(why));
            }
            let made = Made::ParameterRemoved(document.params()[index].name.clone());
            one(Command::RemoveParam(index), made)
        }
        Call::SetUnits(units) => {
            let units = match units {
                Units::Mm => LengthUnit::Mm,
                Units::In => LengthUnit::In,
            };
            one(Command::SetUnits(units), Made::Design)
        }
        Call::SetTolerance { tolerance: text } => {
            let tolerance = tolerance(text, document.units())
                .map_err(|why| Refused::failed(format!("set_tolerance: {why}")))?;
            one(Command::SetTolerance(tolerance), Made::Design)
        }
        Call::SetRollback { before } => {
            let until = match before {
                Some(name) => Some(feature_named(document, name)?),
                None => None,
            };
            one(Command::SetRollback(until), Made::Design)
        }
        Call::AddSketch { plane, name } => super::sketches::add_sketch(doc, plane, name.as_deref()),
        Call::SetSketchPlane { sketch, plane } => {
            super::sketches::set_sketch_plane(doc, sketch, plane)
        }
        Call::AddLink(_) | Call::EditSketch(_) => Err(Refused::failed(
            "internal error: a sketch edit is proposed, not applied",
        )),
        Call::Undo { .. } => Err(Refused::failed(
            "internal error: an undo takes steps back, it makes no commands",
        )),
        Call::SetFeature(args) => super::set::set_feature(doc, args),
        Call::Extrude(args) => super::make::extrude(doc, args),
        Call::Revolve(args) => super::make::revolve(doc, args),
        Call::Combine(args) => super::make::combine(doc, args),
        Call::MoveBody(args) => super::motion::move_body(doc, args),
        Call::Mirror(args) => super::motion::mirror(doc, args),
        Call::Pattern(args) => super::motion::pattern(doc, args),
        Call::Align(args) => super::motion::align(doc, args),
        Call::Scale(args) => super::motion::scale(doc, args),
        Call::Split(args) => super::modify::split(doc, args),
        Call::Shell(args) => super::modify::shell(doc, args),
        Call::OffsetFace(args) => super::modify::offset_face(doc, args),
        Call::Draft(args) => super::modify::draft(doc, args),
        Call::Chamfer(args) => super::blend::chamfer(doc, args),
        Call::Fillet(args) => super::blend::fillet(doc, args),
        Call::Sweep(args) => super::sweep::sweep(doc, args),
        Call::Loft(args) => super::sweep::loft(doc, args),
        Call::DesignSummary(_)
        | Call::Feature { .. }
        | Call::BodyTopology(_)
        | Call::Sketch(_)
        | Call::Measure(_)
        | Call::Render(_) => Err(Refused::failed("internal error: not an edit")),
    }
}

/// Removing `target` without the user's cascade
/// ([`Document::breaking_removal`], as Delete only applies it): refused,
/// naming them, if later features use it, which the cascade would take
/// ([`Document::removal`]).
fn removal(document: &Document, target: Removable) -> Result<(Vec<Command>, Made), Refused> {
    let only = document.breaking_removal(&[target]);
    let all = document.removal(target);
    // By name in the Timeline's order, as the delete prompt lists them,
    // `ids` sorted (a `Removal`'s are).
    let in_order = |ids: &[FeatureId]| -> Vec<String> {
        (document.features().iter())
            .filter(|feature| ids.binary_search(&feature.id).is_ok())
            .map(|feature| feature.name.clone())
            .collect()
    };
    let dependents: Vec<FeatureId> = (all.features.iter())
        .filter(|id| only.features.binary_search(id).is_err())
        .copied()
        .collect();
    if !dependents.is_empty() {
        let why = format!(
            "later features use it: {}. Remove those first, the last first, or change them",
            names(&in_order(&dependents))
        );
        return Err(Refused::NotApplied(why));
    }
    let bodies = (document.bodies().iter())
        .filter(|body| only.bodies.binary_search(&body.id).is_ok())
        .map(|body| body.name.clone())
        .collect();
    let made = Made::Removed {
        features: in_order(&only.features),
        bodies,
    };
    Ok((vec![Command::RemoveOnly(only.features)], made))
}

/// The fit tolerance `text` asks for, in the design's `units`: a length
/// as a field reads it, or micrometres (`"1 µm"`, as `design_summary`
/// shows it), within [`Tolerance`]'s range.
fn tolerance(text: &str, units: LengthUnit) -> Result<Tolerance, String> {
    let text = text.trim();
    let micrometres = (text.strip_suffix("µm")).or_else(|| text.strip_suffix("um"));
    let fit = match micrometres {
        Some(number) => {
            let ask = Ask::number(units, Tolerance::MAX_FIT * 1000.0)
                .positive()
                .at_least(Tolerance::MIN_FIT * 1000.0);
            ExprValue::new(number, &ask)
                .map_err(|why| why.to_string())?
                .value
                / 1000.0
        }
        None => {
            let ask = Ask::length(units, Tolerance::MAX_FIT)
                .positive()
                .at_least(Tolerance::MIN_FIT);
            ExprValue::new(text, &ask)
                .map_err(|why| why.to_string())?
                .value
        }
    };
    Tolerance::new(fit).ok_or_else(|| "the tolerance is from 0.01 µm to 0.1 mm".to_owned())
}

/// The feature named `name`, by its Timeline name.
pub(super) fn feature_named(document: &Document, name: &str) -> Result<FeatureId, Refused> {
    (document.features().iter())
        .find(|feature| feature.name == *name)
        .map(|feature| feature.id)
        .ok_or_else(|| not_named("feature", name))
}

/// The body named `name`, by its Objects name.
pub(super) fn body_named(document: &Document, name: &str) -> Result<BodyId, Refused> {
    (document.bodies().iter())
        .find(|body| body.name == *name)
        .map(|body| body.id)
        .ok_or_else(|| not_named("body", name))
}

/// The index of the parameter named `name`.
fn param_named(document: &Document, name: &str) -> Result<usize, Refused> {
    (document.params().iter())
        .position(|param| param.name == name.trim())
        .ok_or_else(|| not_named("parameter", name))
}

/// The answer to a name found nowhere.
fn not_named(what: &str, name: &str) -> Refused {
    Refused::failed(format!("no {what} is named \"{}\"", varde_mcp::shown(name)))
}

/// `names` for a sentence, at most [`LISTED`] of them, then how many
/// more.
fn names(names: &[String]) -> String {
    let mut said = (names.iter().take(LISTED))
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(", ");
    if let Some(more) = names.len().checked_sub(LISTED).filter(|more| *more > 0) {
        said.push_str(&format!(" and {more} more"));
    }
    said
}

/// The answer to an edit refused, `why`, the design unchanged.
pub(super) fn not_applied(why: impl std::fmt::Display) -> ToolReply {
    ToolReply::error(format!("Not applied: {why}. The design is unchanged"))
}

/// `names` listed, at most [`LISTED`], with how many more there are.
fn listed(names: Vec<Value>) -> Value {
    let total = names.len();
    let mut listing = json!(names.into_iter().take(LISTED).collect::<Vec<_>>());
    if total > LISTED {
        listing = json!({ "items": listing, "total": total });
    }
    listing
}

/// What an edit's result says, for its row's tooltip: the result and
/// what was made, or why not.
pub(super) fn summary(reply: &ToolReply) -> Option<String> {
    let data = reply.structured.as_ref()?;
    let result = data.get("result")?.as_str()?;
    let name = (data
        .get("feature")
        .or(data.get("sketch"))
        .or(data.get("body"))
        .or(data.get("parameter")))
    .and_then(Value::as_str);
    let failures = (data.get("failures"))
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let mut said = result.to_owned();
    if let Some(name) = name {
        said.push_str(&format!(" · {name}"));
    }
    if failures > 0 {
        said.push_str(&format!(" · {failures} failing"));
    }
    if let Some(failed) = data.get("regeneration").and_then(Value::as_str) {
        said.push_str(&format!(" · {failed}"));
    }
    Some(said)
}

impl Doc {
    /// Takes the MCP edit `call`, from `from`: refused, answered that it
    /// changes nothing, queued behind the user's sketch edits waiting on
    /// the solver, or applied, then waiting for regeneration.
    pub(crate) fn mcp_edit(&mut self, call: Call, from: From) {
        // An edit applies only while its call is live, and the bound runs
        // from its arrival at the server.
        // None of its session's come after it yet: those it fails are none.
        let tag = self.mcp.next_tag();
        if from.late(iced::time::Instant::now()) {
            let refused = Refused::not_applied(super::waiting::NOT_APPLIED_LATE);
            return self.edit_not_applied(tag, from, call, refused);
        }
        // One in flight per session, a bounded few behind it.
        if self.mcp.edits_full(&from) {
            let refused = Refused::not_applied(super::waiting::TOO_MANY_QUEUED);
            return self.edit_not_applied(tag, from, call, refused);
        }
        // With the popup up it waits behind the edit asked about,
        // which may queue on the solver only once the user answers:
        // queued now, it would go first, and its commands would be made
        // on the design before that edit. A cancelled popup gives way,
        // those behind it going on first.
        self.drop_cancelled_ask();
        if let Some(ask) = &self.mcp.ask {
            let editing = ask.editing.clone();
            return self.mcp.add(Waiter {
                tag,
                from,
                call,
                waits: Wait::Asking { editing },
            });
        }
        // An undo is checked at its turn only: the session's edits
        // before it may not have applied yet.
        if self.proposing() && !matches!(call, Call::Undo { .. }) {
            // Checked now too, so a name not found or a refusal comes at
            // once, if the model is of the design as it is and no earlier
            // client edit is still to apply (it may make the name); made
            // again in its turn, on the design as it is then.
            if let Some(refused) = self.read_only_refusal() {
                return self.edit_not_applied(tag, from, call, refused);
            }
            if self.model_ready(self.editor.generation()) && !self.mcp.any_unapplied() {
                match plan(self, &call, &from, true) {
                    Err(refused) => return self.edit_not_applied(tag, from, call, refused),
                    Ok(plan) if plan.is_empty() => {
                        return self
                            .mcp
                            .answer(from, call, ToolReply::text(NOTHING_TO_CHANGE));
                    }
                    Ok(_) => {}
                }
            }
        }
        self.client_take_up(tag, from, call);
        self.sync();
    }

    /// Makes `call`'s commands on the design as it is now and applies
    /// them, waiting for regeneration under `tag`: refused, or changing
    /// nothing, it's answered so.
    pub(super) fn client_edit_now(&mut self, tag: u64, from: From, call: Call) {
        if let Some(refused) = self.read_only_refusal() {
            return self.edit_not_applied(tag, from, call, refused);
        }
        // With the popup up (not cancelled) it waits behind the edit
        // asked about, its commands made once that one went: they may
        // name what it makes.
        if let Some(ask) = self.mcp.ask.as_ref().filter(|ask| !ask.cancelled) {
            let editing = ask.editing.clone();
            return self.mcp.add(Waiter {
                tag,
                from,
                call,
                waits: Wait::Asking { editing },
            });
        }
        let plan = match plan(self, &call, &from, false) {
            Ok(plan) => plan,
            Err(refused) => return self.edit_not_applied(tag, from, call, refused),
        };
        if plan.is_empty() {
            return self
                .mcp
                .answer(from, call, ToolReply::text(NOTHING_TO_CHANGE));
        }
        // A cancelled popup gives way; edits behind it went first, so
        // this one's commands are made again on what they left, keeping
        // its place: first among what's queued on the solver (it was at
        // its turn), not at the back.
        if self.drop_cancelled_ask() {
            if self.proposing() {
                self.proposals.wait_client_first(tag);
                return self.mcp.add(Waiter {
                    tag,
                    from,
                    call,
                    waits: Wait::Queued,
                });
            }
            return self.client_turn(tag, from, call);
        }
        // Never under the user's hands: what they have open closes first,
        // or the popup asks them.
        let Some((from, call)) = self.clear_for_client(tag, from, call) else {
            return;
        };
        match plan {
            Plan::Apply(commands, made) => self.client_apply(tag, from, call, made, commands),
            Plan::Propose(proposed) => self.client_propose(tag, from, call, proposed),
            Plan::Undo => self.client_undo(tag, from, call),
        }
    }

    /// The refusal of an edit while the design is read-only, saying
    /// why, if it is.
    fn read_only_refusal(&self) -> Option<Refused> {
        let why = self.read_only.as_ref()?;
        Some(Refused::not_applied(format_args!(
            "the design is read-only ({why})"
        )))
    }

    /// Applies `call`'s `commands` through [`Doc::apply_client`], `made`
    /// what it's to make: refused or changing nothing, it's answered so;
    /// applied, it waits for regeneration under `tag`.
    pub(super) fn client_apply(
        &mut self,
        tag: u64,
        from: From,
        call: Call,
        made: Made,
        commands: Vec<Command>,
    ) {
        let before = self.editor.generation();
        let state = self.editor.state();
        match self.apply_client(commands) {
            Err(why) => {
                // Worded as the Parameters popup words it, for theirs.
                let why = crate::doc::params::refusal(self.editor.document(), &why);
                self.edit_not_applied(tag, from, call, Refused::NotApplied(why))
            }
            Ok(()) if self.editor.generation() == before => {
                self.mcp
                    .answer(from, call, ToolReply::text(NOTHING_TO_CHANGE));
            }
            Ok(()) => {
                self.client_applied(tag, &from);
                self.proposals.client_applied();
                let again = matches!(made, Made::Link { .. }).then(|| made.clone());
                let made = made.result(self);
                let feature = made.get("id").and_then(Value::as_u64);
                (self.mcp).applied(&from, &call, self.editor.revision(), feature);
                self.mcp.add(Waiter {
                    tag,
                    from,
                    call,
                    waits: Wait::Regen {
                        generation: self.editor.generation(),
                        after: self.editor.revision(),
                        made,
                        again,
                        before: super::waiting::Before(state),
                        undid: None,
                    },
                });
            }
        }
    }

    /// Answers the MCP edits waiting on regeneration that the response
    /// applied last reached: at their generation or past it, model or
    /// failure.
    pub(crate) fn edits_regenerated(&mut self) {
        let Some(answered) = self.feed.answered() else {
            return;
        };
        for waiter in self.mcp.regenerated(answered) {
            let Wait::Regen {
                generation,
                after,
                mut made,
                again,
                before,
                undid,
            } = waiter.waits
            else {
                continue;
            };
            // Made again now, over what it said as it applied.
            if let Some(again) = again
                && let (Value::Object(made), Value::Object(now)) = (&mut made, again.result(self))
            {
                made.extend(now);
            }
            let mut reply = self.edit_result(made, generation, after, undid.is_some());
            // An undo the user redid meanwhile, wholly or in part.
            let redone = undid.as_ref().is_some_and(|undid| {
                (undid.afters.iter()).any(|after| self.editor.is_done(self.mcp.amended_to(*after)))
            });
            if redone && let Some(data) = &mut reply.structured {
                data["result"] = json!(super::undo::REDONE);
                reply = ToolReply::data(data.take());
            }
            (self.mcp).answer_applied(waiter.from, waiter.call, reply, (before.0, after), undid);
        }
    }

    /// An applied edit's result, `made` what it made at `generation`,
    /// making `after`: whether the user took it back since, and the
    /// regeneration answer's failures and the features it didn't build.
    /// An `undo` (`undo`) made no step of its own for the user to take
    /// back: their Undo since took back another, and their Redo of what
    /// it undid is told by the caller.
    fn edit_result(
        &self,
        mut made: Value,
        generation: Generation,
        after: Revision,
        undo: bool,
    ) -> ToolReply {
        if !undo && !self.in_effect(after) {
            made["result"] = json!(UNDONE);
            return ToolReply::data(made);
        }
        made["result"] = json!(APPLIED);
        if let Some((_, error)) = self.feed.failure() {
            made["regeneration"] = json!(format!("failed: {error}"));
            return ToolReply::data(made);
        }
        let document = self.editor.document();
        match self.feed.committed() {
            Some(model) if model.generation >= generation => {
                let failures: Vec<Value> = (model.failures.iter())
                    .filter_map(|failure| {
                        let feature = document.feature(failure.feature)?;
                        Some(json!({ "feature": feature.name, "failure": failure.message }))
                    })
                    .collect();
                if !failures.is_empty() {
                    made["failures"] = listed(failures);
                }
                let cut_at = (model.until).and_then(|until| document.feature_index(until));
                if let Some(cut_at) = cut_at {
                    let not_built = (document.features().iter().skip(cut_at))
                        .map(|feature| json!(feature.name))
                        .collect();
                    // Rolled back to the marker, or to a feature the user
                    // has open.
                    made["not_built"] = listed(not_built);
                }
            }
            // Answered with a preview of what the user has open, say.
            _ => {
                made["regeneration"] = json!("not shown yet; see design_summary");
            }
        }
        ToolReply::data(made)
    }
}

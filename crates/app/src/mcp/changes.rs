//! The user's changes since a client's previous call (`notes/AgentSupport.md`,
//! "The user's changes since the client's last call"): what each call's
//! result carries as `user_changes`, and what the MCP panel shows as the
//! user's own rows.
//!
//! Built by diffing the design as the session's previous call left it
//! ([`Seen`], an `Arc` of the document; `Arc::ptr_eq` short-cuts no
//! change) with the design now, by id within a lineage, in the tools'
//! terms. Walked along the editor's history where it can be: the
//! client's own steps on it ([`Waiting::steps`], by after-revision) split
//! it into runs of the user's, each diffed alone, so the client's edits
//! aren't reported back. The user's Undo and Redo of the session's edits
//! are said by name. A design of another lineage (or another design
//! open) is reported replaced; one whose previous state is gone from the
//! history (the user undid past the client's edits, then edited) says so
//! first, and an id there names the same thing only if the state the two
//! branches share has it: one made on each since is removed and added,
//! not changed. The panel's rows leave the reverts made from it out:
//! each is a step of its own there, as the client's are.
//!
//! [`Waiting::steps`]: super::waiting::Waiting::steps

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Map, Value, json};
use varde_document::{Body, BodyId, Document, Feature, FeatureId, FeatureKind, Revision, Snapshot};
use varde_mcp::session::SessionId;

use super::query::{self, Shape};
use super::waiting::ClientStep;
use crate::doc::{Doc, DocId};

/// The most bytes of JSON `user_changes` takes before it names only
/// which features changed, leaving the client to query them.
pub(crate) const CHANGES_BYTES: usize = 4096;

/// The bytes a reply's data leaves under the cap for what [`fitted`]
/// adds to every reply: the revision, and `user_changes` at its
/// briefest ([`briefest`]). Tested to be enough.
pub(crate) const ROOM: usize = 512;

/// The most names the bounded form lists of each kind.
const NAMES: usize = 100;

/// The most of the user's lines the panel shows for one call; the rest
/// counted.
pub(crate) const LINES: usize = 20;

/// What the design was as a session's previous call left it.
#[derive(Debug, Clone)]
pub(crate) struct Seen {
    doc: DocId,
    document: Snapshot,
    revision: Revision,
    lineage: Revision,
    /// The revisions of the history it had then, itself the last: its
    /// ancestors, for finding where a branch since left it.
    ancestors: Vec<Revision>,
}

impl Seen {
    /// `doc`'s design as it is.
    pub(crate) fn of(doc: &Doc) -> Self {
        Self {
            doc: doc.id,
            document: doc.editor.snapshot(),
            revision: doc.editor.revision(),
            lineage: doc.editor.lineage(),
            ancestors: doc.editor.history().map(|(revision, _)| revision).collect(),
        }
    }

    /// Whether `doc`'s design is still as this saw it.
    pub(crate) fn is_now(&self, doc: &Doc) -> bool {
        self.doc == doc.id && self.revision == doc.editor.revision()
    }
}

/// The user's changes since a [`Seen`].
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Changes {
    /// `user_changes`, if there are any.
    pub(crate) value: Option<Value>,
    /// The same at its briefest, for a reply too long to carry `value`
    /// ([`attach`]).
    pub(crate) brief: Option<Value>,
    /// The same, a line each: "renamed Hole 2 to Bolt hole", "reverted
    /// to before ...". The "Told: ..." line under a call.
    pub(crate) lines: Vec<String>,
    /// The panel's rows of them: `lines` but for the reverts made from
    /// the panel and what they changed, which their own rows say.
    pub(crate) panel: Vec<String>,
}

/// The user's changes to `doc`'s design since `seen`, for the session
/// `session` of the server `start` (whose own edits are told apart).
pub(crate) fn since(doc: &Doc, seen: &Seen, start: u64, session: &SessionId) -> Changes {
    let mut changes = said(doc, seen, start, session, false).done();
    let reverts = (doc.mcp.reverted.iter())
        .any(|made| doc.editor.is_done(made.after) && !seen.ancestors.contains(&made.after));
    changes.panel = match (&changes.value, reverts) {
        (None, _) => Vec::new(),
        (Some(_), false) => changes.lines.clone(),
        (Some(_), true) => said(doc, seen, start, session, true).lines,
    };
    changes
}

/// The user's changes to `doc`'s design since `seen`, as [`since`]
/// says them; for the `panel`'s rows, the reverts made from it left
/// out, each a step of its own as the client's are.
fn said(doc: &Doc, seen: &Seen, start: u64, session: &SessionId, panel: bool) -> Said {
    let mut said = Said::default();
    let editor = &doc.editor;
    let now = editor.snapshot();
    if seen.doc != doc.id || seen.lineage != editor.lineage() {
        said.replaced();
        return said;
    }
    // The session's edits the user undid or redid.
    for step in (doc.mcp.steps.iter()).filter(|step| step.of(start, session)) {
        let done = editor.is_done(step.after);
        if done != step.told_done {
            said.step(step, done);
        }
    }
    // The reverts made from the panel since, named as such.
    for made in &doc.mcp.reverted {
        if !panel && editor.is_done(made.after) && !seen.ancestors.contains(&made.after) {
            said.reverted.push(made.to.clone());
            said.lines.push(format!("reverted to {}", made.to));
        }
    }
    if std::sync::Arc::ptr_eq(&seen.document, &now) {
        return said;
    }
    // The state seen, followed through the links' amends since (a reply
    // sent before the regeneration that relinked it) and the session's
    // own undos (not the user's to tell of).
    let revision = followed(doc, seen.revision);
    let history: Vec<_> = editor.history().collect();
    let client = |revision: Revision| {
        doc.mcp.steps.iter().any(|step| step.after == revision)
            || (panel && doc.mcp.reverted.iter().any(|made| made.after == revision))
    };
    // Amended or undone by the client since, from the state it came to.
    let followed_to = editor.state_of(revision);
    let seen_document = match (revision == seen.revision, &followed_to) {
        (false, Some(state)) => state.document(),
        _ => &seen.document,
    };
    if let Some(at) = history.iter().position(|(at, _)| *at == revision) {
        let from = match revision == seen.revision {
            true => &seen.document,
            false => history[at].1,
        };
        said.runs(doc, &history[at..], from, None, &client);
    } else {
        // Undone past it: diffed against the newest state the branch it
        // was on shares with the history. Nothing new since (it's on the
        // redo history), that's the design now and the ids are the same;
        // else the user edited after, and an id made on each branch
        // names two things. The client's steps on the branch since split
        // it into runs as above.
        let shared = (history.iter()).rposition(|(revision, _)| seen.ancestors.contains(revision));
        match shared {
            Some(at) => {
                said.undid = history[at + 1..]
                    .iter()
                    .any(|(revision, _)| !client(*revision));
                let shared: &Document = history[at].1;
                said.runs(doc, &history[at..], seen_document, Some(shared), &client);
            }
            None => said.replaced(),
        }
    }
    said
}

/// The state `revision` came to through the links' amends since and the
/// client's own undos (`Waiting::undos`, while it's still undone): what
/// a session that last saw it is told the user's changes from.
fn followed(doc: &Doc, mut revision: Revision) -> Revision {
    // Each step goes to another state; bounded all the same.
    for _ in 0..2 * super::waiting::STEPS + 1 {
        revision = doc.mcp.amended_to(revision);
        if doc.editor.is_done(revision) {
            break;
        }
        match (doc.mcp.undos.iter()).find(|(from, _)| *from == revision) {
            Some(&(_, to)) => revision = to,
            None => break,
        }
    }
    revision
}

/// What reverting `doc`'s design to `to` would change, a line each,
/// with no author: the revert dialog's list of the later changes it
/// undoes.
pub(crate) fn differences(doc: &Doc, to: &Document) -> Vec<String> {
    let mut said = Said::default();
    said.diff(doc, doc.editor.document(), to, None);
    said.lines
}

/// The changes said so far.
#[derive(Debug, Default)]
struct Said {
    replaced: bool,
    /// The user undid past the client's edits, then edited.
    undid: bool,
    undone: Vec<String>,
    redone: Vec<String>,
    /// The reverts made from the panel: what each went back to.
    reverted: Vec<String>,
    features: Kinds<FeatureChange>,
    bodies: Kinds<BodyChange>,
    parameters: Kinds<ParamChange>,
    design: Map<String, Value>,
    /// The names of the features added, removed or changed, for the
    /// bounded form.
    feature_names: Vec<String>,
    lines: Vec<String>,
}

/// Things of one kind added, removed and changed (`C`, a change).
#[derive(Debug, Serialize)]
struct Kinds<C> {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    added: Vec<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    removed: Vec<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    changed: Vec<C>,
}

impl<C> Default for Kinds<C> {
    fn default() -> Self {
        Self {
            added: Vec::new(),
            removed: Vec::new(),
            changed: Vec::new(),
        }
    }
}

impl<C: Change> Kinds<C> {
    fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }

    /// `change`, if it says one, among the changes, merged into the
    /// entry of the same thing changed in an earlier run if there's one,
    /// as [`Change::then`] merges; the entry dropped if nothing's left.
    fn change(&mut self, change: C) {
        if !change.says() {
            return;
        }
        let Some(at) = self.changed.iter().position(|was| change.follows(was)) else {
            self.changed.push(change);
            return;
        };
        self.changed[at].then(change);
        if !self.changed[at].says() {
            self.changed.remove(at);
        }
    }
}

/// A thing's change, merged with a later one of the same thing.
trait Change {
    /// Whether this is a later change of what `was` changed.
    fn follows(&self, was: &Self) -> bool;
    /// Takes the later change `later`: each field's first old and last
    /// new, a value back where it was dropped. A field something else
    /// moved between the two (a client's edit) is said from where that
    /// left it.
    fn then(&mut self, later: Self);
    /// Whether it says anything changed, beyond what it names.
    fn says(&self) -> bool;
}

/// A value's change: `{"old": old, "new": new}`.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct OldNew<T> {
    old: T,
    new: T,
}

impl<T: PartialEq> OldNew<T> {
    fn new(old: T, new: T) -> Option<Self> {
        (old != new).then_some(Self { old, new })
    }

    /// `field`, a change if any, followed by `later`'s: changed on from
    /// where the earlier left it, from its old to the later's new; else
    /// the later's. None if that's back where it was.
    fn then(field: &mut Option<Self>, later: Option<Self>) {
        let Some(later) = later else { return };
        *field = match field.take() {
            Some(mut was) if was.new == later.old => {
                was.new = later.new;
                (was.old != was.new).then_some(was)
            }
            _ => Some(later),
        };
    }
}

/// A flag said as its new value, `field`, followed by `later`: changed
/// again, it's back where it was, unless something between flipped it
/// too.
fn flag_then(field: &mut Option<bool>, later: Option<bool>) {
    match (*field, later) {
        (Some(was), Some(later)) if was != later => *field = None,
        (None, Some(later)) => *field = Some(later),
        _ => {}
    }
}

/// `{"old": old, "new": new}`.
fn old_new(old: impl Into<Value>, new: impl Into<Value>) -> Value {
    json!({ "old": old.into(), "new": new.into() })
}

/// A feature's change, known by its id.
#[derive(Debug, Serialize)]
struct FeatureChange {
    name: String,
    id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    renamed: Option<OldNew<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    visible: Option<bool>,
    /// The settings changed, by name; empty where its kind changed in
    /// none it says.
    #[serde(skip_serializing_if = "Option::is_none")]
    settings: Option<BTreeMap<String, Setting>>,
}

/// A feature setting's change.
#[derive(Debug, Serialize)]
#[serde(untagged)]
enum Setting {
    Changed(OldNew<Value>),
    /// A sketch's points moved, its counts and plane the same: `"changed"`.
    Geometry(&'static str),
}

impl Change for FeatureChange {
    fn follows(&self, was: &Self) -> bool {
        was.id == self.id
    }

    fn then(&mut self, later: Self) {
        self.name = later.name;
        OldNew::then(&mut self.renamed, later.renamed);
        flag_then(&mut self.visible, later.visible);
        let Some(later) = later.settings else { return };
        let Some(settings) = &mut self.settings else {
            self.settings = Some(later);
            return;
        };
        for (key, value) in later {
            match (settings.remove(&key), value) {
                (Some(Setting::Changed(was)), Setting::Changed(value)) => {
                    let mut field = Some(was);
                    OldNew::then(&mut field, Some(value));
                    if let Some(field) = field {
                        settings.insert(key, Setting::Changed(field));
                    }
                }
                (_, value) => {
                    settings.insert(key, value);
                }
            }
        }
        if settings.is_empty() {
            self.settings = None;
        }
    }

    fn says(&self) -> bool {
        self.renamed.is_some() || self.visible.is_some() || self.settings.is_some()
    }
}

/// A body's change, known by name.
#[derive(Debug, Serialize)]
struct BodyChange {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    renamed: Option<OldNew<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    visible: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    color: Option<OldNew<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    opacity: Option<OldNew<Value>>,
}

impl Change for BodyChange {
    fn follows(&self, was: &Self) -> bool {
        was.name == from_name(&self.name, &self.renamed)
    }

    fn then(&mut self, later: Self) {
        self.name = later.name;
        OldNew::then(&mut self.renamed, later.renamed);
        flag_then(&mut self.visible, later.visible);
        OldNew::then(&mut self.color, later.color);
        OldNew::then(&mut self.opacity, later.opacity);
    }

    fn says(&self) -> bool {
        self.renamed.is_some()
            || self.visible.is_some()
            || self.color.is_some()
            || self.opacity.is_some()
    }
}

/// A parameter's change, known by name.
#[derive(Debug, Serialize)]
struct ParamChange {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    renamed: Option<OldNew<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expression: Option<OldNew<String>>,
}

impl Change for ParamChange {
    fn follows(&self, was: &Self) -> bool {
        was.name == from_name(&self.name, &self.renamed)
    }

    fn then(&mut self, later: Self) {
        self.name = later.name;
        OldNew::then(&mut self.renamed, later.renamed);
        OldNew::then(&mut self.expression, later.expression);
    }

    fn says(&self) -> bool {
        self.renamed.is_some() || self.expression.is_some()
    }
}

/// The name a change of a thing known by name, `name` now, starts from.
fn from_name<'a>(name: &'a str, renamed: &'a Option<OldNew<String>>) -> &'a str {
    renamed.as_ref().map_or(name, |renamed| &renamed.old)
}

impl Said {
    fn replaced(&mut self) {
        self.replaced = true;
        self.lines.push("replaced the design".to_owned());
    }

    /// The session's edit `step` undone, or redone if `done`: by the
    /// tool's name for the client, as its row says it for the panel.
    fn step(&mut self, step: &ClientStep, done: bool) {
        let (labels, did) = match done {
            true => (&mut self.redone, "redid"),
            false => (&mut self.undone, "undid"),
        };
        labels.push(step.label.clone());
        let shown = crate::words::lowercase_first(&step.shown);
        self.lines.push(format!("{did} its edit: {shown}"));
    }

    /// The user's changes from `from` along `history` (from the state
    /// they start at, with its revision), each run of the user's steps
    /// between the client's (those `client` picks by after-revision)
    /// alone: a client's step ends the run before it and starts the
    /// next. `shared` restricts the first run's ids as [`Said::diff`]'s
    /// does.
    fn runs(
        &mut self,
        doc: &Doc,
        history: &[(Revision, &Snapshot)],
        from: &Snapshot,
        mut shared: Option<&Document>,
        client: &dyn Fn(Revision) -> bool,
    ) {
        let mut from = from;
        for pair in history.windows(2) {
            let [(_, before), (revision, after)] = pair else {
                unreachable!("windows of two")
            };
            if client(*revision) {
                if !std::sync::Arc::ptr_eq(from, before) {
                    self.diff(doc, from, before, shared);
                }
                shared = None;
                from = after;
            }
        }
        let now = history.last().map(|(_, now)| *now);
        if let Some(now) = now.filter(|now| !std::sync::Arc::ptr_eq(from, now)) {
            self.diff(doc, from, now, shared);
        }
    }

    /// What changed from `old` to `new`, states of `doc`'s design. With
    /// `shared`, the state the two branch from: an id names the same
    /// thing in both only if it has it.
    fn diff(&mut self, doc: &Doc, old: &Document, new: &Document, shared: Option<&Document>) {
        let same_feature = |id: FeatureId| shared.is_none_or(|shared| shared.feature(id).is_some());
        let same_body = |id: BodyId| shared.is_none_or(|shared| shared.body(id).is_some());
        let matched = |feature: &Feature, other: &Document| -> bool {
            same_feature(feature.id)
                && (other.feature(feature.id))
                    .is_some_and(|other| other.kind.noun() == feature.kind.noun())
        };
        let (was, is) = (Shape::at(doc, old), Shape::at(doc, new));
        for feature in old.features() {
            match new.feature(feature.id).filter(|_| matched(feature, new)) {
                None => {
                    self.features.removed.push(brief(feature));
                    self.feature_named(&feature.name);
                    self.lines.push(format!("removed {}", feature.name));
                }
                Some(now) => self.feature_changed(&was, &is, feature, now),
            }
        }
        for feature in new.features() {
            if !matched(feature, old) {
                let mut added = brief(feature);
                added["settings"] = is.settings(feature);
                self.features.added.push(added);
                self.feature_named(&feature.name);
                self.lines.push(format!("added {}", feature.name));
            }
        }
        for body in old.bodies() {
            match new.body(body.id).filter(|_| same_body(body.id)) {
                None => {
                    self.bodies.removed.push(json!(body.name));
                    self.lines.push(format!("removed body {}", body.name));
                }
                Some(now) => self.body_changed(body, now),
            }
        }
        for body in new.bodies() {
            if old.body(body.id).filter(|_| same_body(body.id)).is_none() {
                self.bodies.added.push(json!(body.name));
                self.lines.push(format!("added body {}", body.name));
            }
        }
        self.parameters_changed(old, new);
        self.design_changed(old, new);
    }

    /// `name` among the features added, removed or changed, once.
    fn feature_named(&mut self, name: &str) {
        if !self.feature_names.iter().any(|named| named == name) {
            self.feature_names.push(name.to_owned());
        }
    }

    fn feature_changed(&mut self, was: &Shape, is: &Shape, old: &Feature, new: &Feature) {
        let mut change = FeatureChange {
            name: new.name.clone(),
            id: new.id.get(),
            renamed: OldNew::new(old.name.clone(), new.name.clone()),
            visible: None,
            settings: None,
        };
        if change.renamed.is_some() {
            self.lines
                .push(format!("renamed {} to {}", old.name, new.name));
        }
        if old.visible != new.visible {
            change.visible = Some(new.visible);
            self.lines.push(format!(
                "{} {}",
                if new.visible { "showed" } else { "hid" },
                new.name
            ));
        }
        if old.kind != new.kind {
            let fields = settings_diff(was, is, old, new);
            let said: Vec<&str> = fields.keys().map(String::as_str).collect();
            let what = match said.is_empty() {
                true => String::new(),
                false => format!(" ({})", said.join(", ")),
            };
            self.lines.push(format!("changed {}{what}", new.name));
            change.settings = Some(fields);
        }
        if change.says() {
            self.features.change(change);
            self.feature_named(&new.name);
        }
    }

    fn body_changed(&mut self, old: &Body, new: &Body) {
        let mut change = BodyChange {
            name: new.name.clone(),
            renamed: OldNew::new(old.name.clone(), new.name.clone()),
            visible: None,
            color: None,
            opacity: None,
        };
        if change.renamed.is_some() {
            self.lines
                .push(format!("renamed body {} to {}", old.name, new.name));
        }
        if old.visible != new.visible {
            change.visible = Some(new.visible);
            self.lines.push(format!(
                "{} {}",
                if new.visible { "showed" } else { "hid" },
                new.name
            ));
        }
        if old.color != new.color {
            change.color = OldNew::new(color(old), color(new));
            self.lines
                .push(format!("changed the colour of {}", new.name));
        }
        if old.opacity != new.opacity {
            change.opacity = OldNew::new(opacity(old), opacity(new));
            self.lines
                .push(format!("changed the opacity of {}", new.name));
        }
        self.bodies.change(change);
    }

    /// Parameters by name; one whose name alone changed at its place, a
    /// rename.
    fn parameters_changed(&mut self, old: &Document, new: &Document) {
        let (was, is) = (old.params(), new.params());
        if was == is {
            return;
        }
        let named = |params: &[varde_document::Param], name: &str| {
            params.iter().position(|param| param.name == name)
        };
        let mut renamed = vec![None; was.len()];
        if was.len() == is.len() {
            for (at, (old, new)) in was.iter().zip(is).enumerate() {
                if old.name != new.name
                    && named(is, &old.name).is_none()
                    && named(was, &new.name).is_none()
                {
                    renamed[at] = Some(at);
                }
            }
        }
        for (at, param) in was.iter().enumerate() {
            let now = renamed[at].or_else(|| named(is, &param.name));
            let Some(now) = now else {
                self.parameters.removed.push(json!(param.name));
                self.lines.push(format!("removed parameter {}", param.name));
                continue;
            };
            let new = &is[now];
            let change = ParamChange {
                name: new.name.clone(),
                renamed: OldNew::new(param.name.clone(), new.name.clone()),
                expression: OldNew::new(param.text.clone(), new.text.clone()),
            };
            if change.renamed.is_some() {
                self.lines
                    .push(format!("renamed parameter {} to {}", param.name, new.name));
            }
            if change.expression.is_some() {
                self.lines.push(format!("set {} to {}", new.name, new.text));
            }
            self.parameters.change(change);
        }
        for (at, param) in is.iter().enumerate() {
            let was_there = renamed.contains(&Some(at)) || named(was, &param.name).is_some();
            if !was_there {
                let row = query::parameter(new, at).unwrap_or_else(|| json!(param.name));
                self.parameters.added.push(row);
                self.lines
                    .push(format!("added parameter {} = {}", param.name, param.text));
            }
        }
    }

    fn design_changed(&mut self, old: &Document, new: &Document) {
        if old.units() != new.units() {
            let (was, is) = (old.units().symbol(), new.units().symbol());
            self.design.insert("units".to_owned(), old_new(was, is));
            self.lines.push(format!("set the units to {is}"));
        }
        let tolerance = |document: &Document| {
            varde_view::tolerance_label(document.tolerance().fit()).to_string()
        };
        let (was, is) = (tolerance(old), tolerance(new));
        if was != is {
            self.lines.push(format!("set the tolerance to {is}"));
            self.design.insert("tolerance".to_owned(), old_new(was, is));
        }
        let rollback = super::query::rollback_name;
        let (was, is) = (rollback(old), rollback(new));
        if was != is {
            self.lines.push(match &is {
                Some(name) => format!("rolled the Timeline back before {name}"),
                None => "rolled the Timeline to the end".to_owned(),
            });
            self.design.insert("rollback".to_owned(), old_new(was, is));
        }
    }

    /// The changes, `user_changes` bounded by [`CHANGES_BYTES`].
    fn done(self) -> Changes {
        let mut value = Map::new();
        if self.replaced {
            value.insert(
                "replaced".to_owned(),
                json!("the design was replaced: query it again"),
            );
        }
        if self.undid {
            value.insert(
                "note".to_owned(),
                json!(
                    "the user undid your edits, then edited: these are the changes from \
                     the design as you last saw it"
                ),
            );
        }
        for (key, labels) in [
            ("undid_your_edits", &self.undone),
            ("redid_your_edits", &self.redone),
            ("reverted", &self.reverted),
        ] {
            if !labels.is_empty() {
                value.insert(key.to_owned(), json!(labels));
            }
        }
        let mut full = value.clone();
        let kinds = [
            ("features", self.features.is_empty(), json!(self.features)),
            ("bodies", self.bodies.is_empty(), json!(self.bodies)),
            (
                "parameters",
                self.parameters.is_empty(),
                json!(self.parameters),
            ),
        ];
        for (key, empty, kinds) in kinds {
            if !empty {
                full.insert(key.to_owned(), kinds);
            }
        }
        full.extend(self.design.clone());
        if full.is_empty() {
            return Changes::default();
        }
        let full = Value::Object(full);
        let brief = briefest(self.replaced);
        let value = match bytes(&full) <= CHANGES_BYTES {
            true => full,
            false => {
                // Named only: the client queries them. As many as fit,
                // the rest counted.
                let mut names = self.feature_names;
                let total = names.len();
                names.truncate(NAMES);
                value.insert(
                    "bounded".to_owned(),
                    json!(
                        "too many changes to list: only the features added, removed or \
                         changed are named; query them, and design_summary for the rest"
                    ),
                );
                let mut labels_left_out = 0usize;
                loop {
                    value.insert(
                        "features_changed".to_owned(),
                        json!({ "names": names, "left_out": total.saturating_sub(names.len()) }),
                    );
                    if labels_left_out > 0 {
                        value.insert("your_edits_left_out".to_owned(), json!(labels_left_out));
                    }
                    let bounded = Value::Object(value.clone());
                    if bytes(&bounded) <= CHANGES_BYTES {
                        break bounded;
                    }
                    if names.pop().is_some() {
                        continue;
                    }
                    // The labels of the client's edits undone or redone,
                    // the longer list's last first.
                    let longest = ["undid_your_edits", "redid_your_edits"]
                        .into_iter()
                        .max_by_key(|key| {
                            value
                                .get(*key)
                                .and_then(Value::as_array)
                                .map_or(0, Vec::len)
                        });
                    match longest
                        .and_then(|key| value.get_mut(key))
                        .and_then(Value::as_array_mut)
                        .and_then(Vec::pop)
                    {
                        Some(_) => labels_left_out = labels_left_out.saturating_add(1),
                        None => break brief.clone(),
                    }
                }
            }
        };
        Changes {
            value: Some(value),
            brief: Some(brief),
            lines: self.lines,
            panel: Vec::new(),
        }
    }
}

/// The bytes of `value`'s JSON.
fn bytes(value: &Value) -> usize {
    serde_json::to_string(value).map_or(usize::MAX, |text| text.len())
}

/// `user_changes` at its briefest, for when the changes don't fit with a
/// reply: that there are some, for the client to query.
pub(super) fn briefest(replaced: bool) -> Value {
    let said = match replaced {
        true => "the design was replaced: query it again",
        false => {
            "the user changed the design: too much to list with this result; call \
                  design_summary, and query what you need"
        }
    };
    json!({ "bounded": said })
}

/// A feature named, as an added or removed one is listed.
fn brief(feature: &Feature) -> Value {
    let mut said = json!({
        "kind": feature.kind.noun(),
        "name": feature.name,
        "id": feature.id.get(),
    });
    if let Some(tool) = query::tool_of(&feature.kind) {
        said["tool"] = json!(tool.name());
    }
    said
}

/// The settings that differ between `old` and `new`, the same feature,
/// each with its old and new value. A sketch changed where its counts and
/// plane didn't (a point moved) says so as `geometry`.
fn settings_diff(
    was: &Shape,
    is: &Shape,
    old: &Feature,
    new: &Feature,
) -> BTreeMap<String, Setting> {
    let (Value::Object(old_settings), Value::Object(new_settings)) =
        (was.settings(old), is.settings(new))
    else {
        return BTreeMap::new();
    };
    let mut fields = BTreeMap::new();
    for (key, value) in &old_settings {
        let now = new_settings.get(key).cloned().unwrap_or(Value::Null);
        if let Some(change) = OldNew::new(value.clone(), now) {
            fields.insert(key.clone(), Setting::Changed(change));
        }
    }
    for (key, value) in &new_settings {
        if !old_settings.contains_key(key) {
            let change = OldNew::new(Value::Null, value.clone());
            fields.extend(change.map(|change| (key.clone(), Setting::Changed(change))));
        }
    }
    if fields.is_empty() && matches!(new.kind, FeatureKind::Sketch { .. }) {
        fields.insert("geometry".to_owned(), Setting::Geometry("changed"));
    }
    fields
}

/// A body's colour as `set_color` takes it, or null for the theme's.
fn color(body: &Body) -> Value {
    match body.color {
        Some(tint) => json!({
            "hue": tint.hue(),
            "saturation": f64::from(tint.saturation()) / 100.0,
        }),
        None => Value::Null,
    }
}

/// A body's opacity as `set_opacity` takes it.
fn opacity(body: &Body) -> Value {
    json!(f64::from(body.opacity.percent()) / 100.0)
}

/// `reply` carrying `changes`, if any, and the design's `revision`: in
/// its structured content (and so its text) when it's data, else as lines
/// after its text (the revision only with changes there).
pub(crate) fn attach(
    reply: varde_mcp::tools::ToolReply,
    changes: Option<Value>,
    revision: Revision,
) -> varde_mcp::tools::ToolReply {
    let revision = serde_json::to_value(revision).unwrap_or(Value::Null);
    let data = reply.structured.as_ref().is_some_and(Value::is_object) && !reply.is_error;
    let fields = match changes {
        Some(changes) => vec![("user_changes", changes), ("revision", revision)],
        None if data => vec![("revision", revision)],
        None => Vec::new(),
    };
    reply.with_fields(fields)
}

/// `reply` carrying `changes` as [`attach`] does, if it then fits
/// [`ToolReply::CAP`](varde_mcp::tools::ToolReply::CAP); else their
/// brief form if that fits, else none of them. Whether the client was
/// told (or there was nothing to tell): a reply that can't carry them
/// leaves them for the next.
pub(crate) fn fitted(
    reply: varde_mcp::tools::ToolReply,
    changes: &Changes,
    revision: Revision,
) -> (varde_mcp::tools::ToolReply, bool) {
    let cap = varde_mcp::tools::ToolReply::CAP;
    if changes.value.is_none() {
        return (attach(reply, None, revision), true);
    }
    for form in [&changes.value, &changes.brief] {
        let told = attach(reply.clone(), form.clone(), revision);
        if told.text.len() <= cap {
            return (told, true);
        }
    }
    (attach(reply, None, revision), false)
}

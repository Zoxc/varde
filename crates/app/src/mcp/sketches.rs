//! The MCP client's sketch edits that aren't drawing: `add_sketch`,
//! `set_sketch_plane` and `add_link`, each what the UI's action makes.
//!
//! A new sketch is `Document::add_sketch`, New sketch's command, at the
//! rollback marker; a plane changed is `Command::SetSketchPlane`, as
//! picking another plane commits it. A face by key goes through
//! `key::found` on the committed model, then the plane pick's own check
//! (`face_placement_in`, which a click on the model shown shares): flat,
//! made before the sketch, not too far out. Neither opens the sketch for
//! the user (the UI's New sketch enters it).
//!
//! A link is the Project or Intersect tool's click: its source named as
//! the tool names a click's (`outside_model_in`, `outside_sketch_item_at`,
//! as of the sketch's place, on the committed model), checked as
//! `propose_with` checks the user's (`SketchEdit::apply` on the
//! committed sketch), then proposed to the solve lane as a client's
//! proposal (`Proposals::propose_client`), the call waiting as
//! `Wait::Solving`; accepted, it's committed as `Command::AddLink`
//! through the client's apply path ([`Doc::client_proposed`]), refused,
//! the call says why. Its points and curves come with the next
//! regeneration's relinking, so its result is made again then.

use serde_json::{Value, json};
use varde_document::{Command, Document, FeatureId, FeatureKind, OutsideRef, Plane};
use varde_mcp::tools::{self, AddLink, Call, ItemKind, LinkFrom, Reference, Worded, WorldPlane};
use varde_sketch::{Id, LinkKind, SketchEdit};
use varde_view::{Picked, PlanePick, SketchItem, Tool};

use super::edit::{Made, Refused, feature_named};
use super::make::{committed_marks, failed};
use super::motion::{origin_plane, said};
use super::waiting::{From, Wait, Waiter};
use crate::doc::{
    Doc, Settled, Step, face_placement_in, outside_model_in, outside_sketch_item_at,
    outside_target_in, sketch_of,
};

/// A sketch edit to propose for the client: of the sketch `feature`.
#[derive(Debug)]
pub(super) struct Proposed {
    pub(super) feature: FeatureId,
    pub(super) what: Proposing,
}

/// What [`Proposed`] proposes.
#[derive(Debug)]
pub(super) enum Proposing {
    /// A link of `kind` from `source` (`add_link`).
    Link { kind: LinkKind, source: OutsideRef },
    /// An `edit_sketch`'s steps, made on the sketch whose `next_id` was
    /// `next_id`, and what it's to make (`Made::Drawn`), its labels'
    /// ids.
    Steps {
        steps: Vec<Step>,
        next_id: u32,
        made: Made,
    },
}

/// `add_sketch {plane, name}`: New sketch's command on `plane`, at the
/// rollback marker, named `name` if given, checked and numbered on a
/// clash as `rename` names ([`Document::new_name`]).
pub(super) fn add_sketch(
    doc: &Doc,
    plane: &Reference<WorldPlane>,
    name: Option<&str>,
) -> Result<(Vec<Command>, Made), Refused> {
    let document = doc.editor.document();
    let plane = sketch_plane(doc, "add_sketch", plane, None)?;
    let mut command = document.add_sketch(plane);
    let mut notes = Vec::new();
    if let (Some(wanted), Command::AddSketch { name, .. }) = (name, &mut command) {
        *name = super::make::given_name(document, "add_sketch", wanted, &mut notes)?;
    }
    let name = match &command {
        Command::AddSketch { name, .. } => name.clone(),
        _ => String::new(),
    };
    Ok((vec![command], Made::NewSketch { name, notes }))
}

/// `set_sketch_plane {sketch, plane}`: the sketch named put on `plane`,
/// as picking another plane commits it. The plane it's on already
/// changes nothing.
pub(super) fn set_sketch_plane(
    doc: &Doc,
    sketch: &str,
    plane: &Reference<WorldPlane>,
) -> Result<(Vec<Command>, Made), Refused> {
    const TOOL: &str = "set_sketch_plane";
    let document = doc.editor.document();
    let feature = sketch_named(document, sketch)?;
    let plane = sketch_plane(doc, TOOL, plane, Some(feature))?;
    let command = Command::SetSketchPlane { feature, plane };
    Ok((vec![command], Made::Sketch(feature)))
}

/// The sketch feature named `name`, refused if it isn't one.
pub(super) fn sketch_named(document: &Document, name: &str) -> Result<FeatureId, Refused> {
    let id = feature_named(document, name)?;
    if sketch_of(document, id).is_none() {
        let why = format!("\"{}\" isn't a sketch", varde_mcp::shown(name));
        return Err(Refused::failed(why));
    }
    Ok(id)
}

/// The plane `plane` names for `tool`: an origin plane, or a face by
/// key found on the committed model, checked as a click picking a plane
/// is ([`face_placement_in`]): for a new sketch, or for the sketch
/// `changed`'s plane.
fn sketch_plane(
    doc: &Doc,
    tool: &str,
    plane: &Reference<WorldPlane>,
    changed: Option<FeatureId>,
) -> Result<Plane, Refused> {
    let item = match plane {
        Reference::World(plane) => return Ok(Plane::Origin(origin_plane(*plane))),
        Reference::Key(item) => item,
    };
    let refused = |why: String| failed(tool, said("plane", why));
    let found = super::key::found(doc, &item.key, ItemKind::Face).map_err(refused)?;
    let Picked::Face(face) = found.pick.target else {
        return Err(refused(
            "internal error: a face's key found something else".into(),
        ));
    };
    let marks = committed_marks(doc).map_err(|why| refused(why.into()))?;
    let document = doc.editor.document();
    let pick = match changed {
        None => Some(PlanePick::new_sketch(document, marks)),
        Some(id) => PlanePick::change(document, id, None, marks),
    };
    let pick = pick.ok_or_else(|| refused("the sketch is gone".into()))?;
    let (face, _) = face_placement_in(&found.index, face, found.pick.at, &pick)
        .map_err(|why| refused(why.into_owned()))?;
    Ok(Plane::Face(face))
}

/// `add_link {sketch, kind?, from}`: what the Project or Intersect
/// tool's click on `from` proposes, or why not, in the tool's words
/// (prefixed with the argument). What a link of the kind comes from
/// already is refused (the tool's click would take that link out).
pub(super) fn add_link(doc: &Doc, args: &AddLink) -> Result<Proposed, Refused> {
    const TOOL: &str = "add_link";
    let document = doc.editor.document();
    let feature = sketch_named(document, &args.sketch)?;
    let (tool, kind) = match args.kind {
        tools::LinkKind::Project => (Tool::Project, LinkKind::Project),
        tools::LinkKind::Intersect => (Tool::Intersect, LinkKind::Intersect),
    };
    let at = document
        .feature_index(feature)
        .unwrap_or(document.insert_at());
    let name = document.features().get(at).map_or("", |f| f.name.as_str());
    let refused = |why: String| failed(TOOL, said("from", why));
    // The model's targets, to tell a source linked already.
    let mut found_on = None;
    let source = match &args.from {
        LinkFrom::Item(item) => {
            let found = super::key::found(doc, &item.key, item.kind).map_err(refused)?;
            let marks = committed_marks(doc).map_err(|why| refused(why.into()))?;
            let naming = doc.naming_in(Some(feature), marks);
            let source = outside_model_in(tool, &found.index, &naming, found.pick, name)
                .map_err(|why| refused(why.into_owned()))?;
            found_on = Some((found.index, found.pick.target));
            source
        }
        LinkFrom::Curve(curve) => {
            let item = item_in(document, &curve.sketch, curve.curve, "curve")?;
            outside_sketch_item_at(document, tool, item, name, at)
                .map_err(|why| refused(why.into_owned()))?
        }
        LinkFrom::Point { sketch, point } => {
            let item = item_in(document, sketch, *point, "point")?;
            outside_sketch_item_at(document, tool, item, name, at)
                .map_err(|why| refused(why.into_owned()))?
        }
    };
    let Some(FeatureKind::Sketch {
        sketch, sources, ..
    }) = document.feature(feature).map(|feature| &feature.kind)
    else {
        return Err(Refused::failed("internal error: the sketch is gone"));
    };
    // Linked already, of this kind: by what it finds on the committed
    // model, as the tool tells a second click on it.
    let merged = committed_marks(doc).map_or(&[][..], |marks| marks.merged);
    let holder = |body| {
        (merged.iter())
            .find(|(merged, _)| *merged == body)
            .map_or(body, |&(_, holder)| holder)
    };
    let linked = (sources.iter()).find(|from| {
        let same_kind = sketch.link(from.link).is_some_and(|link| link.kind == kind);
        let same = match &found_on {
            Some((index, target)) => {
                outside_target_in(index, holder, &from.source) == Some(*target)
            }
            None => from.source == source,
        };
        same_kind && same
    });
    if let Some(linked) = linked {
        return Err(Refused::not_applied(format!(
            "that is linked into {name} already, as link {}",
            linked.link.get()
        )));
    }
    // As the user's proposal is checked before it's proposed.
    let edit = SketchEdit::AddLink { kind };
    if let Err(why) = edit.apply(sketch, &document.design()) {
        return Err(Refused::not_applied(why));
    }
    Ok(Proposed {
        feature,
        what: Proposing::Link { kind, source },
    })
}

/// Another sketch's curve or point (`what`) numbered `number`, of the
/// sketch named `sketch` ([`super::modify::curve_in`],
/// [`super::sweep::point_in`]).
fn item_in(
    document: &Document,
    sketch: &str,
    number: u32,
    what: &str,
) -> Result<SketchItem, Refused> {
    let feature = sketch_named(document, sketch)?;
    let item = match what {
        "curve" => super::modify::curve_in(document, feature, number),
        _ => super::sweep::point_in(document, feature, number),
    };
    let item = item.map_err(|why| failed("add_link", said("from", why)))?;
    Ok(SketchItem {
        sketch: feature,
        item,
    })
}

/// A sketch's result: its name, id and plane.
pub(super) fn sketch_made(doc: &Doc, id: FeatureId) -> Value {
    let Some(feature) = doc.editor.document().feature(id) else {
        return json!({});
    };
    let mut made = json!({ "sketch": feature.name, "id": id.get() });
    if let FeatureKind::Sketch { plane, .. } = &feature.kind {
        made["plane"] = super::query::Shape::new(doc).sketch_plane(plane);
    }
    made
}

/// A link's result: its sketch's as [`sketch_made`], the link's id and
/// kind, and the ids of the points and curves it made, as `sketch`
/// lists them.
pub(super) fn link_made(doc: &Doc, feature: FeatureId, link: Id) -> Value {
    let mut made = sketch_made(doc, feature);
    made["link"] = json!(link.get());
    let sketch = sketch_of(doc.editor.document(), feature);
    if let Some(link) = sketch.and_then(|sketch| sketch.link(link)) {
        let ids = |ids: &[Id]| ids.iter().map(|id| json!(id.get())).collect::<Vec<_>>();
        made["kind"] = json!(
            match link.kind {
                LinkKind::Project => tools::LinkKind::Project,
                LinkKind::Intersect => tools::LinkKind::Intersect,
            }
            .word()
        );
        made["points"] = json!(ids(&link.points));
        made["curves"] = json!(ids(&link.curves));
    }
    made
}

impl Doc {
    /// Proposes the client's sketch edit `proposed` for the call `call`
    /// under `tag`, at its turn, as the user's are proposed (a link as
    /// the Project or Intersect click proposes one, an `edit_sketch` in
    /// its steps): first among what's queued, the call waiting as
    /// [`Wait::Solving`] until the solver answers
    /// ([`Doc::client_proposed`]).
    pub(super) fn client_propose(&mut self, tag: u64, from: From, call: Call, proposed: Proposed) {
        let Proposed { feature, what } = proposed;
        let made = match &what {
            Proposing::Steps { made, .. } => Some(made.clone()),
            Proposing::Link { .. } => None,
        };
        // Waiting before it's sent: a sketch gone answers it at once.
        self.mcp.add(Waiter {
            tag,
            from,
            call,
            waits: Wait::Solving(made),
        });
        let units = self.editor.document().units();
        match what {
            Proposing::Link { kind, source } => {
                let edit = SketchEdit::AddLink { kind };
                (self.proposals).propose_client(tag, feature, edit, Some(source), units);
            }
            Proposing::Steps { steps, next_id, .. } => {
                (self.proposals).propose_steps(tag, feature, steps, units, next_id);
            }
        }
        self.send_proposal();
    }

    /// The solver answered the client's proposal for the call under
    /// `tag`: `settled` the command committing it, applied through the
    /// client's apply path, or why it wasn't accepted, which the call
    /// says. One ended meanwhile (its proposal dropped) isn't answered
    /// again.
    pub(crate) fn client_proposed(&mut self, tag: u64, settled: Result<Settled, String>) {
        let Some(waiter) = (self.mcp.solving(tag))
            .then(|| self.mcp.take(tag))
            .flatten()
            .and_then(|waiter| self.unless_late(waiter))
        else {
            return;
        };
        let Waiter {
            tag,
            from,
            call,
            waits,
        } = waiter;
        let Settled {
            command,
            freedom,
            referenced,
        } = match settled {
            Ok(settled) => settled,
            Err(why) => return self.edit_not_applied(tag, from, call, Refused::not_applied(why)),
        };
        // An `edit_sketch`'s, completed by the solver's answer.
        if let Wait::Solving(Some(Made::Drawn {
            feature, labels, ..
        })) = waits
        {
            let made = Made::Drawn {
                feature,
                labels,
                freedom: Some(freedom),
                referenced,
            };
            return self.client_apply(tag, from, call, made, vec![command]);
        }
        let made = match &command {
            Command::AddLink {
                feature, source, ..
            } => Made::Link {
                feature: *feature,
                link: source.link,
            },
            Command::SetSketch { feature, .. } => Made::Sketch(*feature),
            _ => Made::Design,
        };
        self.client_apply(tag, from, call, made, vec![command]);
    }
}

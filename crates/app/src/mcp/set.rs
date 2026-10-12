//! `set_feature {name, ...patch}`: a patch in the shape `feature` shows
//! (its creating tool's arguments, [`varde_mcp::tools::fields`]) merged
//! into the feature, as its panel's OK on the edited feature commits it
//! (`Command::SetFeature`).
//!
//! The patch is checked against the kind's fields (an unknown one, or
//! one of another kind, is refused) and read by the tool's own parser
//! ([`varde_mcp::tools::parse_patch`]), so a patch for kind K is K's
//! tool arguments all optional; only the fields it gives are taken. A field sent as `feature` shows it
//! changes nothing, so a feature read and sent back whole (references
//! said to be unresolved among it) is nothing to change, unless it goes
//! with one that changes and is read anew with it (a new sketch's
//! profiles, [`GOES_WITH`]). The rest are
//! taken into a session opened on the feature as the panel's
//! EditFeature opens it, through the same setters and click paths the
//! creating tool uses (`make::set_extrude` and the like), and the
//! feature it then holds is the new one. A field of a mode the result
//! doesn't show (an extrude's `second_distance` with one side) is
//! refused, as the panel has no field for it.
//!
//! A list `feature` gave as a page (part of a long one, see its docs) is
//! refused where the list goes ([`page_sent`]), so a feature read and
//! sent back whole never cuts a list down to the page read. A list sent
//! as a plain array is taken as the whole list, as any patch's is:
//! guessing that one equal to some page is a page would refuse a client
//! keeping just those on purpose.

use std::collections::BTreeSet;

use serde_json::{Map, Value};
use varde_document::{Command, FeatureId, FeatureKind};
use varde_mcp::tools::{self, Call, CallError};

use super::edit::{Made, Refused, feature_named};
use super::make::{self, Given, failed};
use super::motion::Wrong;
use super::query::{Shape, tool_of};
use super::{blend, modify, motion, sweep};
use crate::doc::{CombineSession, Doc, ExtrudeSession, MotionSession, RevolveSession};

/// The tool refusals here go under.
const TOOL: &str = make::SET_FEATURE;

/// The fields read anew when the one before them changes, each with
/// those: sent as `feature` shows them, they're taken all the same.
const GOES_WITH: &[(&str, &[&str])] = &[("sketch", &["profiles", "axis"]), ("body", &["from"])];

/// The commands `set_feature` makes on `doc`'s design, and what it's to
/// make: see the module's docs.
pub(super) fn set_feature(
    doc: &Doc,
    args: &tools::SetFeature,
) -> Result<(Vec<Command>, Made), Refused> {
    let document = doc.editor.document();
    let id = feature_named(document, &args.name)?;
    let Some(feature) = document.feature(id) else {
        return Err(failed(
            TOOL,
            "internal error: the feature is gone".to_owned(),
        ));
    };
    let noun = feature.kind.noun().to_lowercase();
    let Some(tool) = tool_of(&feature.kind) else {
        let why = "a sketch's settings aren't set with set_feature: edit its geometry with the \
                   sketch tools";
        return Err(failed(TOOL, why.to_owned()));
    };
    if let Some(why) = tools::renaming(&args.patch) {
        return Err(failed(TOOL, why));
    }
    let fields = tools::fields(tool).unwrap_or_default();
    if let Some(key) = args
        .patch
        .keys()
        .find(|key| !fields.contains(&key.as_str()))
    {
        let why = format!(
            "{} {noun} has no field \"{}\": its fields are {}",
            crate::words::article(&noun),
            varde_mcp::cut(key, 64),
            fields.join(", ")
        );
        return Err(failed(TOOL, why));
    }
    if let Some(key) = args
        .patch
        .iter()
        .find(|(_, value)| value.is_null())
        .map(|(k, _)| k)
    {
        let why = format!("{key} is null: give the value it's to have, as feature shows it");
        return Err(failed(TOOL, why));
    }
    if let Some(key) = page_sent(&args.patch) {
        let why = format!(
            "{} is a page of its list as feature gave it (items, total, left_out), not the \
             list: leave it out to keep the list, or send the whole list it's to be",
            varde_mcp::cut(&key, 64)
        );
        return Err(failed(TOOL, why));
    }
    // A field as `feature` shows it changes nothing: left out.
    let shown = Shape::new(doc).of(feature).unwrap_or_default();
    let mut changed: Map<String, Value> = (args.patch.iter())
        .filter(|(key, value)| shown.get(*key) != Some(*value))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    // But one going with a field that changes is taken with it, as it's
    // read anew there (a new sketch's profiles and axis, a new body's
    // side).
    for (key, with) in GOES_WITH {
        if changed.contains_key(*key) {
            for &with in *with {
                if let Some(value) = args.patch.get(with) {
                    changed.insert(with.to_owned(), value.clone());
                }
            }
        }
    }
    if changed.is_empty() {
        return Ok((Vec::new(), Made::Set(id)));
    }
    let given: BTreeSet<String> = changed.keys().cloned().collect();
    let call = tools::parse_patch(tool, &changed).map_err(|error| {
        let why = match error {
            CallError::Params(why) | CallError::Arguments(why) => why,
        };
        // Worded as the tool's, under this one's name.
        let why = why
            .strip_prefix(&format!("{tool}: "))
            .unwrap_or(&why)
            .to_owned();
        failed(TOOL, why)
    })?;
    let given = Given::Only(&given);
    let kind: FeatureKind = match (&call, &feature.kind) {
        (Call::Extrude(args), FeatureKind::Extrude(extrude)) => {
            let mut session = ExtrudeSession::editing_for_client(document, id, extrude);
            make::set_extrude(doc, &mut session, args, given)?;
            (session.extrude().map(Into::into)).ok_or_else(|| none_picked(&noun))?
        }
        (Call::Revolve(args), FeatureKind::Revolve(revolve)) => {
            let mut session = RevolveSession::editing(document, id, revolve);
            make::set_revolve(doc, &mut session, args, given)?;
            (session.revolve(document).map(Into::into)).ok_or_else(|| none_picked(&noun))?
        }
        (Call::Combine(args), FeatureKind::Combine(combine)) => {
            let mut session = CombineSession::editing(id, combine);
            // As the panel opens it: bodies gone since let go of, to be
            // given again.
            session.prune(document);
            make::set_combine(doc, &mut session, args, given)?;
            let combine = session.combine();
            combine
                .map(Into::into)
                .ok_or_else(|| failed(TOOL, "no tools were given".to_owned()))?
        }
        (call, _) => motion(doc, id, call, given)?,
    };
    let command = Command::SetFeature {
        feature: id,
        kind: Box::new(kind),
    };
    Ok((vec![command], Made::Set(id)))
}

/// The motion-panel kind the feature `id` of `doc`'s design becomes
/// with `call` (its tool's arguments, those `given`) taken into a
/// session opened on it as the panel's EditFeature opens it, checked as
/// its OK checks it.
fn motion(doc: &Doc, id: FeatureId, call: &Call, given: Given) -> Result<FeatureKind, Refused> {
    let document = doc.editor.document();
    let shape = doc.pattern_shapes.get(&id);
    let mut session = MotionSession::editing_for_client(document, id, shape)
        .ok_or_else(|| failed(TOOL, "internal error: another kind's arguments".to_owned()))?;
    // As the panel opens it: what's gone since said to be.
    session.prune(document);
    let session_ = &mut session;
    let set = match call {
        Call::MoveBody(args) => motion::set_move(doc, session_, args, given),
        Call::Mirror(args) => motion::set_mirror(doc, session_, args, given),
        Call::Pattern(args) => motion::set_pattern(doc, session_, args, given),
        Call::Align(args) => motion::set_align(doc, session_, args, given),
        Call::Scale(args) => motion::set_scale(doc, session_, args, given),
        Call::Split(args) => modify::set_split(doc, session_, args, given),
        Call::Shell(args) => modify::set_shell(doc, session_, args, given),
        Call::OffsetFace(args) => modify::set_offset_face(doc, session_, args, given),
        Call::Draft(args) => modify::set_draft(doc, session_, args, given),
        Call::Chamfer(args) => blend::set_chamfer(doc, session_, args, given),
        Call::Fillet(args) => blend::set_fillet(doc, session_, args, given),
        Call::Sweep(args) => sweep::set_sweep(doc, session_, args, given),
        Call::Loft(args) => sweep::set_loft(doc, session_, args, given),
        _ => Err(Wrong::Said(
            "internal error: another kind's arguments".to_owned(),
        )),
    };
    set.map_err(|wrong| wrong.of(TOOL))?;
    let kind = (session.built(&document.design())).map_err(|why| failed(TOOL, why))?;
    if let Some(why) = doc.motion_held_in(&session) {
        return Err(failed(TOOL, why));
    }
    Ok(kind)
}

/// Why the feature set up can't be built: it has no regions, or its
/// axis is gone.
fn none_picked(noun: &str) -> Refused {
    failed(
        TOOL,
        format!("the {noun} has no regions picked or its axis is gone: give profiles (and axis)"),
    )
}

/// The field of `patch` (or `field.inner`, in an object there, as a
/// split's `tool.profiles`) that holds a page as `feature` gives one in
/// place of a list too long to give whole, if one does.
fn page_sent(patch: &Map<String, Value>) -> Option<String> {
    use super::query::is_page;
    patch.iter().find_map(|(key, value)| {
        if is_page(value) {
            return Some(key.clone());
        }
        let inner = value.as_object()?;
        let (sub, _) = inner.iter().find(|(_, value)| is_page(value))?;
        Some(format!("{key}.{sub}"))
    })
}

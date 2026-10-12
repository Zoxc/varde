//! The MCP client's tools for the motion panel's other kinds, `split`,
//! `shell`, `offset_face` and `draft` (see `notes/AgentSupport.md`, "Edits"):
//! each set up in the panel's own session
//! ([`MotionSession::for_client`]) as the motion tools are (see
//! `motion`), built from what its panel shows (typed text read by its
//! fields, bodies by name as clicks pick them, faces by key taken as
//! clicks on them in the committed model are, a sketch's regions and
//! curves as its clicks pick them), its defaults the panel's, and added
//! where its OK adds it, at the rollback marker. What the panel refuses
//! before OK (its need, its own check, a value its field refuses, a
//! choice it doesn't offer) the call refuses, saying why; what it leaves
//! to the document the editor refuses.

use varde_document::{Command, Keep, MAX_EXTRUDE_REGIONS, Side};
use varde_mcp::tools::{self, Reference, SplitWith};
use varde_view::{MotionField, MotionKind, MotionPick, ShellDirection, SplitMode};

use super::args::{
    Given, SET_FEATURE, Wrong, by_key, committed_marks, curve_in, faces, faces_given, failed,
    goes_with, keyed, made, made_of, origin_plane, pick, picked_bodies, said, sketch_named,
    tool_for, typed_given,
};
use super::edit::{Made, Refused};
use crate::doc::{Doc, MotionSession, RegionPick};

/// The commands `split` makes on `doc`'s design, and what it's to make:
/// its body by name as a click picks it, then its tool as its tile picks
/// it (an origin plane as the toolbar's buttons, a face by key as a click
/// on it, another body by name, a sketch's regions as an extrude's, a
/// line's curves by id as clicks on them), which piece keeps the body's
/// id only while both are kept (the panel's Keeps tiles are off for a
/// trim).
pub(super) fn split(doc: &Doc, args: &tools::Split) -> Result<(Vec<Command>, Made), Refused> {
    let names = std::slice::from_ref(&args.body);
    made(doc, "split", MotionKind::Split, names, |session| {
        set_split(doc, session, args, Given::All)
    })
}

/// Takes `args`, those `given`, into the split `session`: a tool a patch
/// gives replaces its tile's own, picked afresh.
pub(super) fn set_split(
    doc: &Doc,
    session: &mut MotionSession,
    args: &tools::Split,
    given: Given,
) -> Result<(), Wrong> {
    let document = doc.editor.document();
    let edited = session.feature;
    let tool = tool_for("split", edited);
    if edited.is_some() && given.has("body") {
        let names = std::slice::from_ref(&args.body);
        session.bodies = picked_bodies(doc, tool, "split", names, edited)?;
    }
    if given.has("tool") {
        let mode = match &args.tool {
            SplitWith::Surface(_) => SplitMode::Face,
            SplitWith::Body(_) => SplitMode::Body,
            SplitWith::Regions { .. } => SplitMode::Regions,
            SplitWith::Line { .. } => SplitMode::Line,
        };
        session.split_mode(mode, document);
        if edited.is_some() {
            match mode {
                SplitMode::Face => session.split.surface = None,
                SplitMode::Body => session.split.body = None,
                SplitMode::Regions => {}
                SplitMode::Line => session.split.chain = None,
            }
            session.picking = MotionPick::Tool;
        }
        match &args.tool {
            SplitWith::Surface(Reference::World(plane)) => {
                session.split_origin(origin_plane(*plane));
            }
            SplitWith::Surface(Reference::Key(item)) => {
                by_key(doc, &item.key, item.kind, "tool", |index, pick| {
                    doc.pick_split_tool(session, index, pick)
                })?;
            }
            SplitWith::Body(name) => {
                let names = std::slice::from_ref(name);
                let picked = picked_bodies(doc, tool, "split", names, edited)?;
                let &[body] = picked.as_slice() else {
                    return Err(said("tool", "no body was given").into());
                };
                let tool = (doc.split_tool_body(session, body)).map_err(|why| said("tool", why))?;
                session.take_split_tool(tool);
            }
            SplitWith::Regions { sketch, profiles } => {
                let sketch = sketch_named(document, tool, sketch, edited)?;
                let regions = &mut session.split.regions;
                *regions = RegionPick::new(Some(sketch), MAX_EXTRUDE_REGIONS);
                pick(regions, document, sketch, profiles)
                    .map_err(|why| said("tool.profiles", why))?;
            }
            SplitWith::Line { sketch, curves } => {
                let id = sketch_named(document, tool, sketch, edited)?;
                for (at, &number) in curves.iter().enumerate() {
                    if curves[..at].contains(&number) {
                        let why = format!("curve {number} is named twice");
                        return Err(said("tool.curves", why).into());
                    }
                    let curve =
                        curve_in(document, id, number).map_err(|why| said("tool.curves", why))?;
                    session.split_curve(id, curve, document);
                }
            }
        }
    }
    let keep = given.or("keep", split_keep(args.keep), session.split.keep);
    if let Some(original) = args.original.filter(|_| given.has("original")) {
        if keep != Keep::Both {
            return Err(goes_with(
                "original",
                "keep both: a trim's kept side keeps the body",
            ));
        }
        session.split.original = match original {
            tools::SplitSide::Front => Side::Front,
            tools::SplitSide::Back => Side::Back,
        };
    }
    session.split.keep = keep;
    Ok(())
}

/// The commands `shell` makes on `doc`'s design, and what it's to make:
/// its faces by key, its body theirs; with none, the body named, or the
/// model's only body as the panel starts with it.
pub(super) fn shell(doc: &Doc, args: &tools::Shell) -> Result<(Vec<Command>, Made), Refused> {
    const TOOL: &str = "shell";
    let set = |session: &mut MotionSession| set_shell(doc, session, args, Given::All);
    match &args.body {
        // The panel takes a body only while it has no faces: theirs is
        // its.
        Some(_) if !args.faces.is_empty() => Err(failed(TOOL, NO_BODY.to_owned())),
        Some(name) => made(
            doc,
            TOOL,
            MotionKind::Shell,
            std::slice::from_ref(name),
            set,
        ),
        None if args.faces.is_empty() => {
            let marks = committed_marks(doc).map_err(|why| failed(TOOL, why))?;
            let only = doc.only_body_in(marks).into_iter().collect();
            made_of(doc, TOOL, MotionKind::Shell, only, set)
        }
        None => made_of(doc, TOOL, MotionKind::Shell, Vec::new(), set),
    }
}

/// Why a shell's body can't be named with faces.
const NO_BODY: &str = "body goes with no faces: a shell's body is its faces'";

/// Takes `args`, those `given`, into the shell `session`; a new one's
/// body was picked as it was opened. Edited, faces given replace its
/// own, their body its; a body only with none.
pub(super) fn set_shell(
    doc: &Doc,
    session: &mut MotionSession,
    args: &tools::Shell,
    given: Given,
) -> Result<(), Wrong> {
    let edited = session.feature;
    if edited.is_some() {
        let faceless = given.or(
            "faces",
            args.faces.is_empty(),
            session.faces.refs.is_empty(),
        );
        if given.has("body") && !faceless {
            return Err(Wrong::Said(NO_BODY.to_owned()));
        }
        if given.has("faces") {
            session.faces = Default::default();
            if !args.faces.is_empty() {
                session.bodies.clear();
            }
            session.refs_body();
        }
        if let Some(name) = args.body.as_ref().filter(|_| given.has("body")) {
            let names = std::slice::from_ref(name);
            session.bodies = picked_bodies(doc, SET_FEATURE, "shell", names, edited)?;
        }
    }
    if given.has("faces") {
        faces(doc, session, &args.faces)?;
    }
    typed_given(
        doc,
        session,
        given,
        MotionField::Thickness,
        &args.thickness,
        "thickness",
    )?;
    if given.has("direction") {
        session.direction = match args.direction {
            tools::ShellDirection::Inward => ShellDirection::Inward,
            tools::ShellDirection::Outward => ShellDirection::Outward,
        };
    }
    Ok(())
}

/// The commands `offset_face` makes on `doc`'s design, and what it's to
/// make: its faces by key, its distance, Inward and Tangent faces.
pub(super) fn offset_face(
    doc: &Doc,
    args: &tools::OffsetFace,
) -> Result<(Vec<Command>, Made), Refused> {
    made_of(
        doc,
        "offset_face",
        MotionKind::OffsetFace,
        Vec::new(),
        |session| set_offset_face(doc, session, args, Given::All),
    )
}

/// Takes `args`, those `given`, into the offset face `session`.
pub(super) fn set_offset_face(
    doc: &Doc,
    session: &mut MotionSession,
    args: &tools::OffsetFace,
    given: Given,
) -> Result<(), Wrong> {
    faces_given(doc, session, &args.faces, given)?;
    typed_given(
        doc,
        session,
        given,
        MotionField::Distance,
        &args.distance,
        "distance",
    )?;
    given.set("inward", &mut session.flip, args.inward);
    given.set("tangent", &mut session.tangent, args.tangent);
    Ok(())
}

/// The commands `draft` makes on `doc`'s design, and what it's to make:
/// its faces by key, then its neutral plane, an origin plane as the
/// toolbar's buttons take it or a flat face by key as a click on it is
/// taken, refused if it's one of the faces drafted (as the panel refuses
/// a click on one); its angle, Flip and Tangent faces.
pub(super) fn draft(doc: &Doc, args: &tools::Draft) -> Result<(Vec<Command>, Made), Refused> {
    made_of(doc, "draft", MotionKind::Draft, Vec::new(), |session| {
        set_draft(doc, session, args, Given::All)
    })
}

/// Takes `args`, those `given`, into the draft `session`.
pub(super) fn set_draft(
    doc: &Doc,
    session: &mut MotionSession,
    args: &tools::Draft,
    given: Given,
) -> Result<(), Wrong> {
    faces_given(doc, session, &args.faces, given)?;
    match args.neutral.as_ref().filter(|_| given.has("neutral")) {
        None => {}
        Some(Reference::World(plane)) => session.origin_plane(origin_plane(*plane)),
        Some(Reference::Key(item)) => keyed(doc, session, item, "neutral")?,
    }
    typed_given(
        doc,
        session,
        given,
        MotionField::Angle,
        &args.angle,
        "angle",
    )?;
    given.set("flip", &mut session.flip, args.flip);
    given.set("tangent", &mut session.tangent, args.tangent);
    Ok(())
}

/// The panel's side to keep for the tools' `keep`.
fn split_keep(keep: tools::SplitKeep) -> Keep {
    match keep {
        tools::SplitKeep::Both => Keep::Both,
        tools::SplitKeep::Front => Keep::Front,
        tools::SplitKeep::Back => Keep::Back,
    }
}

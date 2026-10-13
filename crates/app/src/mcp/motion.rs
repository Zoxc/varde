//! The MCP client's motion tools, `move_body`, `mirror`, `pattern`,
//! `align` and `scale` (see `agents/mcp.md`, "Edits"): each set up in
//! the motion panel's own session ([`MotionSession::for_client`]), built
//! from what its panel shows (typed text read by its fields, bodies by
//! name as clicks pick them, faces, edges and corners by key taken as
//! clicks on them in the committed model are), its defaults the panel's,
//! and added where its OK adds it, at the rollback marker. What the
//! panel refuses before OK (its need, its own check, a value its field
//! refuses, a choice it doesn't offer) the call refuses, saying why;
//! what it leaves to the document the editor refuses.
//!
//! Each tool's setter (`set_move` and the like) takes the arguments
//! [`Given`] into a session: all of them into a new one, or those a
//! `set_feature` patch gives into one opened on the feature, as the
//! panel's EditFeature opens it. A list a patch gives replaces the one
//! the feature has (cleared, then picked again, as the clicks toggle),
//! and a value of a mode the result doesn't show is refused, as the
//! panel has no field for it.

use varde_document::{Axis3, Command};
use varde_mcp::tools::{self, FeatureTool, Item, ItemKind, Reference, WorldAxis};
use varde_view::{
    AlignRole, AlignSide, AlignSlot, MotionField, MotionKind, MotionPick, PatternMode, ScaleMode,
};

use super::args::{
    Given, SET_FEATURE, Wrong, by_key, gated, goes_with, keyed, made, origin_plane, picked_bodies,
    said, typed_given, world_axis,
};
use super::edit::{Made, Refused};
use crate::doc::{Doc, MotionSession};

/// The commands `move_body` makes on `doc`'s design, and what it's to
/// make: see the module's docs.
pub(super) fn move_body(
    doc: &Doc,
    args: &tools::MoveBody,
) -> Result<(Vec<Command>, Made), Refused> {
    made(
        doc,
        "move_body",
        MotionKind::Move,
        &args.bodies,
        |session| set_move(doc, session, args, Given::All(FeatureTool::MoveBody)),
    )
}

/// Takes `args`, those `given`, into the move `session`.
pub(super) fn set_move(
    doc: &Doc,
    session: &mut MotionSession,
    args: &tools::MoveBody,
    given: Given,
) -> Result<(), Wrong> {
    bodies_given(doc, session, &args.bodies, given)?;
    for (axis, text, said) in [
        (Axis3::X, &args.offset[0], "x"),
        (Axis3::Y, &args.offset[1], "y"),
        (Axis3::Z, &args.offset[2], "z"),
    ] {
        typed_given(doc, session, given, MotionField::Offset(axis), text, said)?;
    }
    typed_given(
        doc,
        session,
        given,
        MotionField::Angle,
        &args.angle,
        "angle",
    )?;
    if let Some(axis) = args.axis.as_ref().filter(|_| given.has("axis")) {
        motion_axis(doc, session, axis)?;
    }
    Ok(())
}

/// The commands `mirror` makes on `doc`'s design, and what it's to make.
pub(super) fn mirror(doc: &Doc, args: &tools::Mirror) -> Result<(Vec<Command>, Made), Refused> {
    made(doc, "mirror", MotionKind::Mirror, &args.bodies, |session| {
        set_mirror(doc, session, args, Given::All(FeatureTool::Mirror))
    })
}

/// Takes `args`, those `given`, into the mirror `session`.
pub(super) fn set_mirror(
    doc: &Doc,
    session: &mut MotionSession,
    args: &tools::Mirror,
    given: Given,
) -> Result<(), Wrong> {
    bodies_given(doc, session, &args.bodies, given)?;
    if given.has("plane") {
        match &args.plane {
            Reference::World(plane) => session.origin_plane(origin_plane(*plane)),
            Reference::Key(item) => keyed(doc, session, item, "plane")?,
        }
    }
    given.set(
        "keep_original",
        &mut session.keep_original,
        args.keep_original,
    );
    Ok(())
}

/// The commands `pattern` makes on `doc`'s design, and what it's to
/// make: a linear or circular one as its panel sets it up, its mode
/// offered by its kind, a spread only with a mode that reads one, Flip
/// only for a linear one.
pub(super) fn pattern(doc: &Doc, args: &tools::Pattern) -> Result<(Vec<Command>, Made), Refused> {
    made(
        doc,
        "pattern",
        pattern_kind(args.kind),
        &args.bodies,
        |session| set_pattern(doc, session, args, Given::All(FeatureTool::Pattern)),
    )
}

/// The motion kind of the tools' pattern kind.
fn pattern_kind(kind: tools::PatternKind) -> MotionKind {
    match kind {
        tools::PatternKind::Linear => MotionKind::LinearPattern,
        tools::PatternKind::Circular => MotionKind::CircularPattern,
    }
}

/// Takes `args`, those `given`, into the pattern `session`: an edited
/// one keeps its kind, as the panel edits it as the kind it is.
pub(super) fn set_pattern(
    doc: &Doc,
    session: &mut MotionSession,
    args: &tools::Pattern,
    given: Given,
) -> Result<(), Wrong> {
    let kind = session.kind;
    if given.has("kind") && pattern_kind(args.kind) != kind {
        let why = "a pattern's kind stays as it is: remove it and add one of the other kind";
        return Err(Wrong::Said(why.to_owned()));
    }
    bodies_given(doc, session, &args.bodies, given)?;
    if let Some(axis) = args.axis.as_ref().filter(|_| given.has("axis")) {
        motion_axis(doc, session, axis)?;
    }
    if let Some(mode) = args.mode.filter(|_| given.has("mode")) {
        let mode = match mode {
            tools::PatternMode::Spacing => PatternMode::Spacing,
            tools::PatternMode::Total => PatternMode::Total,
            tools::PatternMode::Full => PatternMode::Full,
        };
        if !session.set_mode(mode) {
            return Err(goes_with("mode full", "kind circular only"));
        }
    }
    if given.sent("flip", args.flip) && kind == MotionKind::CircularPattern {
        return Err(goes_with("flip", "kind linear only"));
    }
    given.set("flip", &mut session.flip, args.flip);
    given.set("join", &mut session.join, args.join);
    typed_given(
        doc,
        session,
        given,
        MotionField::Count,
        &args.count,
        "count",
    )?;
    if given.sent("spread", args.spread.is_some()) && session.mode == PatternMode::Full {
        return Err(goes_with(
            "spread",
            "mode spacing or total: full spreads the copies round a whole turn",
        ));
    }
    typed_given(
        doc,
        session,
        given,
        MotionField::Spread,
        &args.spread,
        "spread",
    )
}

/// The commands `align` makes on `doc`'s design, and what it's to make:
/// its body by name, or the one its point is on; each side's point, then
/// directions, picked in the order the panel's clicks go to them (the
/// body's point, the target's, the body's direction, the target's, the
/// second directions), so a round edge's centre gives its side the rim's
/// axis as the panel's pick does, a direction given replacing it.
pub(super) fn align(doc: &Doc, args: &tools::Align) -> Result<(Vec<Command>, Made), Refused> {
    let names: Vec<String> = args.body.iter().cloned().collect();
    made(doc, "align", MotionKind::Align, &names, |session| {
        set_align(doc, session, args, Given::All(FeatureTool::Align))
    })
}

/// Takes `args`, those `given`, into the align `session`: a side a
/// patch gives replaces the feature's whole, and another body moved
/// needs the moved side given with it, as its references are on the
/// body.
pub(super) fn set_align(
    doc: &Doc,
    session: &mut MotionSession,
    args: &tools::Align,
    given: Given,
) -> Result<(), Wrong> {
    let edited = session.feature;
    if let Some(name) = args
        .body
        .as_ref()
        .filter(|_| edited.is_some() && given.has("body"))
    {
        let names = std::slice::from_ref(name);
        let bodies = picked_bodies(doc, SET_FEATURE, "align", names, edited)?;
        if bodies != session.bodies && !given.has("from") {
            let why = "body goes with from: the moved side's references are on the body moved";
            return Err(Wrong::Said(why.to_owned()));
        }
        session.bodies = bodies;
    }
    let (from, to) = (&args.from, &args.to);
    let sides = [(AlignSide::Moved, "from"), (AlignSide::Target, "to")];
    let taken = |side: AlignSide| sides.iter().any(|&(s, key)| s == side && given.has(key));
    if edited.is_some() {
        for (side, _) in sides.into_iter().filter(|&(side, _)| taken(side)) {
            for role in [AlignRole::Point, AlignRole::Primary, AlignRole::Secondary] {
                session.align.clear(AlignSlot::new(side, role));
            }
        }
        session.picking = session.align.next();
    }
    let slot = AlignSlot::new;
    let points = [
        (AlignSide::Moved, &from.point, "from.point"),
        (AlignSide::Target, &to.point, "to.point"),
    ];
    for (side, point, said) in points.into_iter().filter(|&(side, ..)| taken(side)) {
        let slot = slot(side, AlignRole::Point);
        match point {
            Reference::World(()) => align_origin(session, slot, None, said)?,
            Reference::Key(item) => align_key(doc, session, slot, item, said)?,
        }
    }
    let (primary, secondary) = (AlignRole::Primary, AlignRole::Secondary);
    let directions = [
        (AlignSide::Moved, primary, &from.direction, "from.direction"),
        (AlignSide::Target, primary, &to.direction, "to.direction"),
        (
            AlignSide::Moved,
            secondary,
            &from.second_direction,
            "from.second_direction",
        ),
        (
            AlignSide::Target,
            secondary,
            &to.second_direction,
            "to.second_direction",
        ),
    ];
    for (side, role, direction, said) in directions.into_iter().filter(|&(side, ..)| taken(side)) {
        let slot = slot(side, role);
        match direction {
            None => {}
            Some(Reference::World(axis)) => {
                align_origin(session, slot, Some(world_axis(*axis)), said)?;
            }
            Some(Reference::Key(item)) => align_key(doc, session, slot, item, said)?,
        }
    }
    // The panel stores Flip only with directions: given without them,
    // it's said rather than dropped.
    let directed = (session.align.sides.iter()).all(|side| side.primary.is_some());
    if given.has("flip") && args.flip && !directed {
        return Err(goes_with("flip", "a direction on each side"));
    }
    given.set("flip", &mut session.flip, args.flip);
    typed_given(
        doc,
        session,
        given,
        MotionField::Distance,
        &args.distance,
        "distance",
    )?;
    typed_given(
        doc,
        session,
        given,
        MotionField::Angle,
        &args.angle,
        "angle",
    )
}

/// The commands `scale` makes on `doc`'s design, and what it's to make:
/// about the origin or a point by key, in one of the panel's modes, each
/// value only with the mode that reads it; Along its axis only only for
/// an edge the panel offers it for.
pub(super) fn scale(doc: &Doc, args: &tools::Scale) -> Result<(Vec<Command>, Made), Refused> {
    made(doc, "scale", MotionKind::Scale, &args.bodies, |session| {
        set_scale(doc, session, args, Given::All(FeatureTool::Scale))
    })
}

/// Takes `args`, those `given`, into the scale `session`: each value
/// only with the mode the scale then has.
pub(super) fn set_scale(
    doc: &Doc,
    session: &mut MotionSession,
    args: &tools::Scale,
    given: Given,
) -> Result<(), Wrong> {
    bodies_given(doc, session, &args.bodies, given)?;
    if given.has("point") {
        match &args.point {
            Some(Reference::Key(item)) => {
                by_key(doc, &item.key, item.kind, "point", |index, pick| {
                    doc.pick_scale_point(session, index, pick)
                })?;
            }
            // The origin, as the toolbar's button takes it.
            _ if session.feature.is_some() => {
                let picking = session.picking;
                session.picking = MotionPick::Point;
                session.scale_origin();
                session.picking = picking;
            }
            _ => {}
        }
    }
    let mode = given.or("mode", scale_mode(args.mode), session.scale.mode);
    let [x, y, z] = &args.factors;
    let sent = [
        ("factor", args.factor.is_some(), tools::ScaleMode::Uniform),
        ("x", x.is_some(), tools::ScaleMode::PerAxis),
        ("y", y.is_some(), tools::ScaleMode::PerAxis),
        ("z", z.is_some(), tools::ScaleMode::PerAxis),
        ("edge", args.edge.is_some(), tools::ScaleMode::EdgeLength),
        (
            "length",
            args.length.is_some(),
            tools::ScaleMode::EdgeLength,
        ),
        ("axis_only", args.axis_only, tools::ScaleMode::EdgeLength),
    ];
    gated(given, mode, "mode", &sent, scale_mode)?;
    if given.has("mode") {
        session.scale_mode(mode);
    }
    typed_given(
        doc,
        session,
        given,
        MotionField::Factor,
        &args.factor,
        "factor",
    )?;
    for (axis, text, name) in [(Axis3::X, x, "x"), (Axis3::Y, y, "y"), (Axis3::Z, z, "z")] {
        typed_given(
            doc,
            session,
            given,
            MotionField::AxisFactor(axis),
            text,
            name,
        )?;
    }
    if let Some(text) = args.edge.as_ref().filter(|_| given.has("edge")) {
        let (found, ()) = by_key(doc, text, ItemKind::Edge, "edge", |index, pick| {
            doc.pick_scale_edge(session, index, pick)
        })?;
        // The panel offers Along its axis only for a straight edge
        // along a world axis, as regenerating tells one (the mesh's
        // ends here).
        let along = match found.pick.target {
            varde_view::Picked::Edge(chain) => (found.index.chain_keys(chain))
                .and_then(|keys| found.index.edge_ends(chain, &keys))
                .is_some_and(|[from, to]| varde_regen::along_axis(to - from).is_some()),
            _ => false,
        };
        // Along its axis only kept from the feature goes as given.
        let axis_only = given.or("axis_only", args.axis_only, session.scale.axis_only);
        if axis_only && !along {
            return Err(goes_with("axis_only", "a straight edge along a world axis"));
        }
    }
    given.set("axis_only", &mut session.scale.axis_only, args.axis_only);
    typed_given(
        doc,
        session,
        given,
        MotionField::Length,
        &args.length,
        "length",
    )
}

/// Takes the bodies `names` into the edited `session`, if a patch gives
/// them, picked afresh as clicks on them pick them; a new one's were
/// picked as it was opened.
fn bodies_given(
    doc: &Doc,
    session: &mut MotionSession,
    names: &[String],
    given: Given,
) -> Result<(), Wrong> {
    let edited = session.feature;
    if edited.is_some() && given.has("bodies") {
        let noun = session.kind.noun().to_lowercase();
        session.bodies = picked_bodies(doc, SET_FEATURE, &noun, names, edited)?;
    }
    Ok(())
}

/// Takes `axis` as a move's or pattern's axis: a world axis as the
/// toolbar's buttons take it, or an edge or face by key as a click on it
/// is taken.
fn motion_axis(
    doc: &Doc,
    session: &mut MotionSession,
    axis: &Reference<WorldAxis>,
) -> Result<(), String> {
    match axis {
        Reference::World(axis) => {
            session.origin_axis(world_axis(*axis));
            Ok(())
        }
        Reference::Key(item) => keyed(doc, session, item, "axis"),
    }
}

/// Takes the face, edge or corner `item` names as `slot` of the align
/// `session`, as a click on it in the committed model is taken
/// ([`Doc::pick_align`]).
fn align_key(
    doc: &Doc,
    session: &mut MotionSession,
    slot: AlignSlot,
    item: &Item,
    said: &str,
) -> Result<(), String> {
    by_key(doc, &item.key, item.kind, said, |index, pick| {
        doc.pick_align(session, index, slot, pick)
    })?;
    Ok(())
}

/// Takes the origin, or its axis `axis`, as `slot` of the align
/// `session`, as the toolbar's buttons do: only what it's aligned to.
fn align_origin(
    session: &mut MotionSession,
    slot: AlignSlot,
    axis: Option<Axis3>,
    said: &str,
) -> Result<(), String> {
    if session.align_origin(slot, axis) {
        return Ok(());
    }
    Err(self::said(
        said,
        "the origin and its axes can only be what it's aligned to, in to",
    ))
}

/// The panel's scale mode for the tools' `mode`.
fn scale_mode(mode: tools::ScaleMode) -> ScaleMode {
    match mode {
        tools::ScaleMode::Uniform => ScaleMode::Uniform,
        tools::ScaleMode::PerAxis => ScaleMode::PerAxis,
        tools::ScaleMode::EdgeLength => ScaleMode::EdgeLength,
    }
}

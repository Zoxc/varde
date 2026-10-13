//! The MCP client's tools for the motion panel's Create kinds, `sweep`
//! and `loft` (see `agents/mcp.md`, "Edits"): each set up in the
//! panel's own session
//! ([`MotionSession::for_client`](crate::doc::MotionSession::for_client))
//! as the motion tools are (see `motion`), built from what its panel
//! shows (regions picked as its clicks pick them, a sketch's curves by
//! id as a click on one picks the chain it's in, model edges and an
//! axis by key as clicks on them in the committed model are, sketch
//! points by id, typed text read by its fields), its defaults the
//! panel's but for a helix's pitch, which follows the view, and added
//! where its OK adds it, at the rollback marker. What the panel refuses
//! before OK (its need, its own check, what the document refuses its
//! names at its place, a value its field refuses, a choice it doesn't
//! show) the call refuses, saying why; what it leaves to the document
//! the editor refuses.

use varde_document::{Command, Document, FeatureId, LoftMode, MAX_SWEEP_REGIONS, Section};
use varde_mcp::tools::{self, FeatureTool, Profiles, Reference, SectionShape, SweepAlong};
use varde_view::{MotionField, MotionKind, OperationKind, SweepPath};

use super::args::{
    Given, Wrong, curve_in, edges, excluded, goes_with, keyed, made_of, operation, pick, point_in,
    regions, said, sketch_before, tool_for, typed, typed_given, world_axis,
};
use super::edit::{Made, Refused};
use crate::doc::{Doc, MotionSession, RegionPick};

/// The commands `sweep` makes on `doc`'s design, and what it's to make:
/// its profile's regions as an extrude's, then what it runs along, by
/// the panel's Along tiles: a path's chains of sketch curves (each the
/// chain a click on the curve named picks, of a sketch made before but
/// the profile's) and model edges (as a blend's, the Tangent chain tick
/// stored), or a helix's axis (a world axis as the toolbar's buttons
/// take it, or a straight or round edge or round face by key as a click
/// on it is taken), pitch, turns, Left-handed and Flip; Keep orientation
/// and the twist with a path only (the panel shows them for one); the
/// operation and bodies left out as an extrude's.
pub(super) fn sweep(doc: &Doc, args: &tools::Sweep) -> Result<(Vec<Command>, Made), Refused> {
    made_of(doc, "sweep", MotionKind::Sweep, Vec::new(), |session| {
        set_sweep(doc, session, args, Given::All(FeatureTool::Sweep))
    })
}

/// Takes `args`, those `given`, into the sweep `session`: a path or
/// helix a patch gives replaces what the edited one runs along, Keep
/// orientation and the twist only with a path.
pub(super) fn set_sweep(
    doc: &Doc,
    session: &mut MotionSession,
    args: &tools::Sweep,
    given: Given,
) -> Result<(), Wrong> {
    let document = doc.editor.document();
    let edited = session.feature;
    let tool = tool_for("sweep", edited);
    match edited {
        None => {
            let sketch = sketch_before(document, tool, &args.sketch, None)?;
            let regions = &mut session.sweep.regions;
            *regions = RegionPick::new(Some(sketch), MAX_SWEEP_REGIONS);
            pick(regions, document, sketch, &args.profiles).map_err(|why| said("profiles", why))?;
        }
        Some(_) => regions(
            doc,
            &mut session.sweep.regions,
            tool,
            edited,
            &args.sketch,
            &args.profiles,
            given,
        )?,
    }
    let along = given.has("path") || given.has("helix");
    let path = match (along, &args.along) {
        (true, SweepAlong::Path { .. }) => SweepPath::Path,
        (true, SweepAlong::Helix { .. }) => SweepPath::Helix,
        (false, _) => session.sweep.path,
    };
    if path == SweepPath::Helix {
        for (name, present) in [
            ("keep_orientation", args.keep_orientation),
            ("twist", args.twist.is_some()),
        ] {
            if given.sent(name, present) {
                return Err(goes_with(name, "a path: a helix's profile turns with it"));
            }
        }
    }
    if along {
        if edited.is_some() {
            session.sweep.chains.clear();
            session.sweep.stored_edges.clear();
            session.blend.edges = Default::default();
        }
        match &args.along {
            SweepAlong::Path {
                curves,
                edges: keys,
                tangent,
            } => {
                session.sweep_mode(SweepPath::Path);
                session.blend.chains = *tangent;
                for (at, chain) in curves.iter().enumerate() {
                    let argument = format!("path.curves[{at}]");
                    let id = sketch_before(document, tool, &chain.sketch, edited)?;
                    let curve =
                        curve_in(document, id, chain.curve).map_err(|why| said(&argument, why))?;
                    let before = session.sweep.chains.len();
                    (session.sweep_curve(id, curve, document))
                        .map_err(|why| said(&argument, why))?;
                    // A click on a part's curve takes the part out.
                    if session.sweep.chains.len() < before {
                        let why = "that curve is in the chain of one named before";
                        return Err(said(&argument, why).into());
                    }
                }
                edges(doc, session, keys, "path.edges")?;
            }
            SweepAlong::Helix {
                axis,
                pitch,
                turns,
                left_handed,
                flip,
            } => {
                session.sweep_mode(SweepPath::Helix);
                match axis {
                    Reference::World(axis) => session.origin_axis(world_axis(*axis)),
                    Reference::Key(item) => keyed(doc, session, item, "helix.axis")?,
                }
                typed(doc, session, MotionField::Pitch, pitch, "helix.pitch")?;
                typed(doc, session, MotionField::Turns, turns, "helix.turns")?;
                session.sweep.left_handed = *left_handed;
                session.flip = *flip;
            }
        }
    }
    given.set(
        "keep_orientation",
        &mut session.sweep.keep_orientation,
        args.keep_orientation,
    );
    typed_given(
        doc,
        session,
        given,
        MotionField::Twist,
        &args.twist,
        "twist",
    )?;
    given.set(
        "operation",
        &mut session.sweep.operation,
        operation(args.operation),
    );
    if given.has("exclude") {
        let new_body = session.sweep.operation == OperationKind::NewBody;
        session.sweep.targets = excluded(doc, tool, &args.exclude, new_body, edited)?;
    }
    Ok(())
}

/// The commands `loft` makes on `doc`'s design, and what it's to make:
/// its sections in the order given, each a region (of any sketch made
/// before, shown or not, as the panel takes a section's own) added as a
/// click on it adds it, starting at the corner nearest the previous
/// section's start or at the point given (moved there as a click on that
/// corner moves it), or a point on its own added as a click on it adds
/// it (first or last); then put in the order given as the rows' up
/// chevrons move them. Smooth or Ruled, Closed (before the rails, so a
/// rail with it is refused as the panel refuses its click), the rails
/// as clicks on their curves pick them, the operation and bodies left
/// out as an extrude's.
pub(super) fn loft(doc: &Doc, args: &tools::Loft) -> Result<(Vec<Command>, Made), Refused> {
    made_of(doc, "loft", MotionKind::Loft, Vec::new(), |session| {
        set_loft(doc, session, args, Given::All(FeatureTool::Loft))
    })
}

/// Takes `args`, those `given`, into the loft `session`: sections or
/// rails a patch gives replace the edited one's own.
pub(super) fn set_loft(
    doc: &Doc,
    session: &mut MotionSession,
    args: &tools::Loft,
    given: Given,
) -> Result<(), Wrong> {
    let document = doc.editor.document();
    let edited = session.feature;
    let tool = tool_for("loft", edited);
    if given.has("sections") {
        if edited.is_some() {
            session.loft.sections.clear();
        }
        let mut wanted: Vec<Section> = Vec::with_capacity(args.sections.len());
        for (at, section) in args.sections.iter().enumerate() {
            let argument = format!("sections[{at}]");
            let sketch = sketch_before(document, tool, &section.sketch, edited)?;
            let before = session.loft.sections.clone();
            match &section.shape {
                SectionShape::Region(profile) => {
                    let region = region_of(document, sketch, profile)
                        .map_err(|why| said(&format!("{argument}.region"), why))?;
                    let regions = &mut session.loft.regions;
                    if !regions.also.contains(&sketch) {
                        regions.also.push(sketch);
                    }
                    regions.refresh(document);
                    if regions.found(sketch).is_none() {
                        let why = "the sketch is too complex for its regions to be found";
                        return Err(said(&argument, why).into());
                    }
                    (doc.loft_region_in(session, sketch, region))
                        .map_err(|why| said(&argument, why))?;
                }
                SectionShape::Point(number) => {
                    let point = point_in(document, sketch, *number)
                        .map_err(|why| said(&format!("{argument}.point"), why))?;
                    (session.loft_point(sketch, point, document))
                        .map_err(|why| said(&argument, why))?;
                }
            }
            let sections = &session.loft.sections;
            // A click on a section's region or point takes it out.
            let Some(added) = (0..sections.len()).find(|&k| !before.contains(&sections[k])) else {
                let why = "that section is named twice";
                return Err(said(&argument, why).into());
            };
            if let Some(number) = section.start {
                let said_start = format!("{argument}.start");
                let point =
                    point_in(document, sketch, number).map_err(|why| said(&said_start, why))?;
                session.loft_start(added, point, document);
                let moved = matches!(
                    session.loft.sections[added],
                    Section::Region { start: Some(start), .. } if start == point
                );
                if !moved {
                    let why = format!("point {number} isn't at one of the region's corners");
                    return Err(said(&said_start, why).into());
                }
            }
            wanted.push(session.loft.sections[added].clone());
        }
        // In the order given, as the rows' up chevrons move them.
        for (at, section) in wanted.iter().enumerate() {
            let found = (session.loft.sections.iter().skip(at)).position(|s| s == section);
            let Some(mut from) = found.map(|found| found + at) else {
                continue;
            };
            while from > at {
                session.section_up(from);
                from -= 1;
            }
        }
    }
    if given.has("mode") {
        session.loft.mode = match args.mode {
            tools::LoftMode::Smooth => LoftMode::Smooth,
            tools::LoftMode::Ruled => LoftMode::Ruled,
        };
    }
    if given.has("closed") && session.loft.closed != args.closed {
        session.loft_closed();
    }
    if given.has("rails") {
        if edited.is_some() {
            session.loft.rails.clear();
        }
        for (at, rail) in args.rails.iter().enumerate() {
            let argument = format!("rails[{at}]");
            let sketch = sketch_before(document, tool, &rail.sketch, edited)?;
            let curve =
                curve_in(document, sketch, rail.curve).map_err(|why| said(&argument, why))?;
            let before = session.loft.rails.len();
            (session.loft_rail(sketch, curve, document)).map_err(|why| said(&argument, why))?;
            // A click on a rail's curve takes the rail out.
            if session.loft.rails.len() < before {
                let why = "that curve is in a rail named before";
                return Err(said(&argument, why).into());
            }
        }
    }
    given.set(
        "operation",
        &mut session.loft.operation,
        operation(args.operation),
    );
    if given.has("exclude") {
        let new_body = session.loft.operation == OperationKind::NewBody;
        session.loft.targets = excluded(doc, tool, &args.exclude, new_body, edited)?;
    }
    Ok(())
}

/// The index of the region `profile` names among `sketch`'s, as an
/// extrude's profiles are picked ([`pick`]).
fn region_of(
    document: &Document,
    sketch: FeatureId,
    profile: &tools::Profile,
) -> Result<usize, String> {
    let mut one = RegionPick::new(Some(sketch), 1);
    pick(
        &mut one,
        document,
        sketch,
        &Profiles::Picked(vec![profile.clone()]),
    )?;
    (one.picked.first().copied()).ok_or_else(|| "no region was picked".to_owned())
}

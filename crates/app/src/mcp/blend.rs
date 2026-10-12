//! The MCP client's blend tools, `chamfer` and `fillet` (see
//! `notes/AgentSupport.md`, "Edits"): each set up in the motion panel's own
//! session ([`MotionSession::for_client`]) as the motion tools are (see
//! `motion`), its edges and faces by key taken as clicks on them in the
//! committed model are (an edge with its tangent chain while Tangent
//! chain is on, each edge of its own, as the panel's tick has a click
//! pick them), its values typed into its fields, its defaults the
//! panel's but for the sizes that follow the view, and added where its
//! OK adds it, at the rollback marker. The kernel blends nothing yet, so
//! each applies and fails in regeneration, as for the user.

use varde_document::Command;
use varde_mcp::tools::{self, Worded};
use varde_view::{ChamferType, MotionField, MotionKind};

use super::args::{Given, Wrong, edges, faces, goes_with, made_of, typed_given};
use super::edit::{Made, Refused};
use crate::doc::{Doc, MotionSession};

/// The commands `chamfer` makes on `doc`'s design, and what it's to
/// make: its edges and faces, its type, the values that type reads
/// (each refused with another), Flip sides with two distances or a
/// distance and angle only (the panel shows it for those), Tangent
/// chain.
pub(super) fn chamfer(doc: &Doc, args: &tools::Chamfer) -> Result<(Vec<Command>, Made), Refused> {
    made_of(doc, "chamfer", MotionKind::Chamfer, Vec::new(), |session| {
        set_chamfer(doc, session, args, Given::All)
    })
}

/// Takes `args`, those `given`, into the chamfer `session`: each value
/// only with the type it then has.
pub(super) fn set_chamfer(
    doc: &Doc,
    session: &mut MotionSession,
    args: &tools::Chamfer,
    given: Given,
) -> Result<(), Wrong> {
    let kind = match given.has("type") {
        true => match args.kind {
            tools::ChamferType::Equal => ChamferType::Equal,
            tools::ChamferType::TwoDistances => ChamferType::Two,
            tools::ChamferType::DistanceAngle => ChamferType::Angle,
        },
        false => session.chamfer_type,
    };
    let sent = [
        (
            "second_distance",
            args.second_distance.is_some(),
            ChamferType::Two,
            tools::ChamferType::TwoDistances,
        ),
        (
            "angle",
            args.angle.is_some(),
            ChamferType::Angle,
            tools::ChamferType::DistanceAngle,
        ),
    ];
    if let Some(&(name, _, _, word)) =
        (sent.iter()).find(|&&(name, present, reads, _)| given.sent(name, present) && reads != kind)
    {
        return Err(goes_with(name, &format!("type {}", word.word())));
    }
    if given.sent("flip", args.flip) && kind == ChamferType::Equal {
        let why = format!(
            "type {} or {}: {} is the same either way round",
            tools::ChamferType::TwoDistances.word(),
            tools::ChamferType::DistanceAngle.word(),
            tools::ChamferType::Equal.word(),
        );
        return Err(goes_with("flip", &why));
    }
    lists(doc, session, args.tangent, &args.edges, &args.faces, given)?;
    session.chamfer_type = kind;
    for (field, text, said) in [
        (MotionField::ChamferDistance, &args.distance, "distance"),
        (
            MotionField::ChamferSecond,
            &args.second_distance,
            "second_distance",
        ),
        (MotionField::ChamferAngle, &args.angle, "angle"),
    ] {
        typed_given(doc, session, given, field, text, said)?;
    }
    if given.has("flip") {
        session.flip = args.flip;
    }
    Ok(())
}

/// The commands `fillet` makes on `doc`'s design, and what it's to
/// make: its edges and faces, its radius, Tangent chain.
pub(super) fn fillet(doc: &Doc, args: &tools::Fillet) -> Result<(Vec<Command>, Made), Refused> {
    made_of(doc, "fillet", MotionKind::Fillet, Vec::new(), |session| {
        set_fillet(doc, session, args, Given::All)
    })
}

/// Takes `args`, those `given`, into the fillet `session`.
pub(super) fn set_fillet(
    doc: &Doc,
    session: &mut MotionSession,
    args: &tools::Fillet,
    given: Given,
) -> Result<(), Wrong> {
    lists(doc, session, args.tangent, &args.edges, &args.faces, given)?;
    typed_given(
        doc,
        session,
        given,
        MotionField::Radius,
        &args.radius,
        "radius",
    )
}

/// Takes a blend's Tangent chain tick `tangent`, then its edges
/// `edge_keys` and faces `face_keys` picked as clicks on them pick them,
/// those `given`: a list a patch gives replaces the edited one's own.
fn lists(
    doc: &Doc,
    session: &mut MotionSession,
    tangent: bool,
    edge_keys: &[String],
    face_keys: &[String],
    given: Given,
) -> Result<(), Wrong> {
    if given.has("tangent") {
        session.blend.chains = tangent;
    }
    if session.feature.is_some() {
        if given.has("edges") {
            session.blend.edges = Default::default();
        }
        if given.has("faces") {
            session.faces = Default::default();
        }
        session.refs_body();
    }
    if given.has("edges") {
        edges(doc, session, edge_keys, "edges")?;
    }
    if given.has("faces") {
        faces(doc, session, face_keys)?;
    }
    Ok(())
}

//! The view's per-kind texts of a feature: its icon, its Timeline note
//! and its status bar info, each one match over [`FeatureKind`] handing
//! off to the kind's own module, and the angle and length notes those
//! modules share.

use varde_document::{Document, EdgeRef, FaceRef, Feature, FeatureKind, blend_excludes};
use varde_expr::LengthUnit;

use crate::document::sketch_summary;
use crate::icons::Icon;
use crate::words::counted;

/// An angle of `radians`, as angles are shown: "90°".
pub(crate) fn angle_note(radians: f64) -> String {
    varde_expr::Shown::current().format(radians, Some(varde_expr::AngleUnit::Deg.into()))
}

/// A length in `units`: "10 mm".
pub(crate) fn length_note(value: &varde_expr::Value, units: LengthUnit) -> String {
    varde_expr::Shown::current().format(value.value, Some(units.into()))
}

/// The icon of `feature`, in the Timeline and wherever it's listed.
pub(crate) fn feature_icon(feature: &Feature) -> Icon {
    match feature.kind {
        FeatureKind::Sketch { .. } => Icon::Sketch,
        FeatureKind::Extrude(_) => Icon::Extrude,
        FeatureKind::Revolve(_) => Icon::Revolve,
        FeatureKind::Combine(_) => Icon::Combine,
        FeatureKind::Move(_) | FeatureKind::Mirror(_) | FeatureKind::Pattern(_) => {
            crate::MotionKind::of(&feature.kind).map_or(Icon::Move, crate::MotionKind::icon)
        }
        FeatureKind::Align(_) => Icon::Align,
        FeatureKind::Scale(_) => Icon::Scale,
        FeatureKind::Split(_) => Icon::Split,
        FeatureKind::Chamfer(_) => Icon::BChamfer,
        FeatureKind::Shell(_) => Icon::Shell,
        FeatureKind::Fillet(_) => Icon::BFillet,
        FeatureKind::OffsetFace(_) => Icon::OffsetFace,
        FeatureKind::FaceDraft(_) => Icon::Draft,
        FeatureKind::Sweep(_) => Icon::Sweep,
        FeatureKind::Loft(_) => Icon::Loft,
    }
}

/// A feature's Timeline note, after its name: a sketch's plane
/// ([`plane_note`](crate::plane_note)), an extrude's distances in
/// `units`, how far a revolve turns in all, ...
pub(crate) fn feature_note(document: &Document, feature: &Feature, units: LengthUnit) -> String {
    match &feature.kind {
        FeatureKind::Sketch { plane, .. } => crate::plane_note(document, plane),
        FeatureKind::Extrude(extrude) => crate::extrude::extrude_note(extrude, units),
        FeatureKind::Revolve(revolve) => crate::revolve::turn_note(&revolve.extent),
        FeatureKind::Combine(combine) => combine.op.label().into(),
        FeatureKind::Move(moved) => crate::motion::move_note(moved, units),
        FeatureKind::Mirror(mirror) => crate::motion::plane_short(document, &mirror.plane),
        FeatureKind::Pattern(pattern) => crate::motion::pattern_note(pattern),
        FeatureKind::Align(align) => crate::motion::align_note(document, align),
        FeatureKind::Scale(scale) => crate::motion::scale_note(scale, units),
        FeatureKind::Split(split) => crate::motion::split_note(document, split),
        FeatureKind::Chamfer(chamfer) => crate::chamfer::chamfer_note(chamfer, units),
        FeatureKind::Shell(shell) => crate::shell::shell_note(shell, units),
        FeatureKind::Fillet(fillet) => crate::fillet::fillet_note(fillet, units),
        FeatureKind::OffsetFace(offset) => crate::offset_face::offset_note(offset, units),
        FeatureKind::FaceDraft(draft) => crate::face_draft::draft_note(draft),
        FeatureKind::Sweep(sweep) => crate::sweep::sweep_note(document, sweep),
        FeatureKind::Loft(loft) => crate::loft::loft_note(loft),
    }
}

/// The status bar's info on the selected `feature` of `document`, after
/// its name: a sketch's curves and plane, "4 lines · 1 circle · 5 points
/// · on XY", an extrude's extent in the document's units and operation,
/// "Distance 10 mm · New body", or a revolve's turn, operation and axis,
/// "One side 90° · Join · about Line 3" (the axis left out while its
/// sketch doesn't have it; last, as the box clips what doesn't fit and
/// a line's name says least).
pub(crate) fn feature_info(feature: &Feature, document: &Document) -> String {
    let units = document.units();
    match &feature.kind {
        FeatureKind::Sketch { plane, sketch, .. } => {
            let on = crate::plane_pick::on_plane(document, plane);
            format!("{} · {on}", sketch_summary(sketch))
        }
        FeatureKind::Extrude(extrude) => crate::extrude::extrude_info(extrude, units),
        FeatureKind::Revolve(revolve) => crate::revolve::revolve_info(document, revolve),
        FeatureKind::Combine(combine) => crate::combine::combine_info(document, combine),
        FeatureKind::Move(moved) => crate::motion::move_info(document, moved),
        FeatureKind::Mirror(mirror) => crate::motion::mirror_info(document, mirror),
        FeatureKind::Pattern(pattern) => crate::motion::pattern_info(document, pattern),
        FeatureKind::Align(align) => crate::motion::align_info(document, align),
        FeatureKind::Scale(scale) => crate::motion::scale_info(document, scale, units),
        FeatureKind::Split(split) => crate::motion::split_info(document, split),
        FeatureKind::Chamfer(chamfer) => crate::chamfer::chamfer_info(chamfer, units),
        FeatureKind::Shell(shell) => crate::shell::shell_info(shell, units),
        FeatureKind::Fillet(fillet) => crate::fillet::fillet_info(fillet, units),
        FeatureKind::OffsetFace(offset) => crate::offset_face::offset_info(offset, units),
        FeatureKind::FaceDraft(draft) => crate::face_draft::draft_info(document, draft),
        FeatureKind::Sweep(sweep) => crate::sweep::sweep_info(document, sweep),
        FeatureKind::Loft(loft) => crate::loft::loft_info(loft),
    }
}

/// How many faces and edges a chamfer or fillet names, for the status
/// bar: "2 edges", "1 face · 1 edge", and those it leaves out, "1 face ·
/// 2 left out".
pub(crate) fn blend_count(edges: &[EdgeRef], faces: &[FaceRef]) -> String {
    let out = (edges.iter())
        .filter(|edge| blend_excludes(edge, faces))
        .count();
    let taken = edges.len() - out;
    let mut parts = Vec::new();
    if !faces.is_empty() {
        parts.push(counted(faces.len(), "face", "faces"));
    }
    if taken > 0 || faces.is_empty() {
        parts.push(counted(taken, "edge", "edges"));
    }
    if out > 0 {
        parts.push(format!("{out} left out"));
    }
    parts.join(" · ")
}

/// A chamfer's or fillet's status bar words for its Tangent chain tick:
/// " · Tangent chain" while on.
pub(crate) fn chain_note(chains: bool) -> &'static str {
    if chains { " · Tangent chain" } else { "" }
}

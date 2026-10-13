//! Finding the axes and planes features name, on the bodies as the
//! features before them leave them, worded per feature, and the cache
//! key every reference on a body is filed under.

use glam::DVec3;
use varde_document::{AxisRef, EdgeRef, FaceRef, FeatureId, Plane};
use varde_kernel_compat::measure::{EdgeShape, edge_shape};
use varde_kernel_compat::mesh::Form;
use varde_kernel_compat::topology::Unresolved;
use varde_kernel_compat::{Solid, Tolerance};

use super::{Evaluation, Failed, chain_curves, chain_runs_with, face_geometry};
use crate::cache::{Cache, Key, Keyer};
use crate::picking::region_form;
use crate::why;

/// Notes `reference`, the axis or plane `feature` found, in
/// `evaluation` for its draft's reply, if the workers' wire takes it
/// ([`crate::drafted::reference_fits`]): one that's out of bounds there (a round
/// edge's centre far out, an axis of no length) isn't drawn, natively
/// as on the web.
pub(super) fn note_reference(
    evaluation: &mut Evaluation,
    feature: FeatureId,
    reference: [DVec3; 2],
) {
    if crate::drafted::reference_fits(&reference.map(|v| v.to_array())) {
        evaluation.references.push((feature, reference));
    }
}

/// The line `axis` names, as a point on it and its direction (not unit,
/// not zero), on the bodies as the features before the one naming it
/// leave them (`evaluation`), or why there's none. An origin axis is
/// through the origin. An edge or a face is looked for on its body
/// through [`Evaluation::holder`] (a body with no solid is "gone") and
/// found on its solid's topology by its names and point, as a revolve's
/// axis edge is ([`super::revolve::edge_axis`]): an edge must be straight (the
/// line through it, directed as [`EdgeRef`] says) or round (its circle's
/// axis through its centre, turning the way the edge runs as so
/// directed); a face must be round, a cylinder, cone, torus or other
/// surface of revolution (its form's axis, directed as the form has it,
/// through the point of it nearest the face's point: [`beside`](varde_kernel_compat::topology::beside)).
/// Cached by the solid's key, the reference and the fit tolerance.
pub(super) fn resolve_axis(
    axis: &AxisRef,
    evaluation: &Evaluation,
    tolerance: &Tolerance,
    cache: &mut Cache,
) -> Result<[DVec3; 2], Failed> {
    match axis {
        AxisRef::Origin(axis) => Ok([DVec3::ZERO, axis.direction()]),
        AxisRef::Edge(edge) => {
            let made = evaluation
                .holding(edge.body)
                .ok_or(why::Picked::AxisEdge.body_gone())?;
            let key = reference_key("axis edge", made.key, &edge.faces, edge.near, tolerance);
            cache.reference(key, || edge_line(&made.solid, edge, tolerance))
        }
        AxisRef::Face(face) => {
            let made = evaluation
                .holding(face.body)
                .ok_or(why::Picked::AxisFace.body_gone())?;
            let key = reference_key("axis face", made.key, &face.key, face.near, tolerance);
            cache.reference(key, || face_axis(&made.solid, face, tolerance))
        }
    }
}

/// What a feature calls the plane it names, for its failures: the
/// cache key's kind, and what the face is to the words of its failures.
pub(super) struct PlaneWords {
    pub(super) kind: &'static str,
    pub(super) picked: why::Picked,
}

/// A split's plane: "its plane face wasn't found".
pub(super) const SPLIT_PLANE: PlaneWords = PlaneWords {
    kind: "split plane face",
    picked: why::Picked::PlaneFace,
};

/// A mirror's plane: "its mirror face wasn't found".
pub(super) const MIRROR_PLANE: PlaneWords = PlaneWords {
    kind: "mirror face",
    picked: why::Picked::MirrorFace,
};

/// A draft's neutral plane: "its neutral face wasn't found".
pub(super) const NEUTRAL_PLANE: PlaneWords = PlaneWords {
    kind: "neutral face",
    picked: why::Picked::NeutralFace,
};

/// The plane `plane` names, as a point on it and its normal (not unit,
/// not zero), as [`resolve_axis`] finds a line: an origin plane through
/// the origin; a face must be flat (its form's plane, its normal out of
/// the solid: then unit). Failures are worded by `words`.
pub(super) fn resolve_plane(
    plane: &Plane,
    words: &PlaneWords,
    evaluation: &Evaluation,
    tolerance: &Tolerance,
    cache: &mut Cache,
) -> Result<[DVec3; 2], Failed> {
    match plane {
        Plane::Origin(origin) => Ok([DVec3::ZERO, origin.placement().normal]),
        Plane::Face(face) => {
            let made = evaluation
                .holding(face.body)
                .ok_or(words.picked.body_gone())?;
            let key = reference_key(words.kind, made.key, &face.key, face.near, tolerance);
            cache.reference(key, || {
                face_plane(&made.solid, face, words, tolerance).map(|(n, d)| [n * d, n])
            })
        }
    }
}

/// The key of a reference of `kind` to the faces named `names` near
/// `near` on the solid filed under `solid`, at `tolerance`.
pub(super) fn reference_key(
    kind: &str,
    solid: Key,
    names: &impl serde::Serialize,
    near: DVec3,
    tolerance: &Tolerance,
) -> Key {
    let mut keyer = Keyer::new(kind);
    keyer.key(solid).value(names).point(near);
    keyer.number(tolerance.fit().to_bits()).finish()
}

/// The line through the edge `edge` names on `solid`, or its circle's
/// axis, see [`resolve_axis`].
fn edge_line(solid: &Solid, edge: &EdgeRef, tolerance: &Tolerance) -> Result<[DVec3; 2], Failed> {
    let topology = solid.topology();
    let chain = (topology.edge(solid, edge.faces, edge.near))
        .map_err(|_| why::Picked::AxisEdge.not_found())?;
    let chain = &topology.chains()[chain as usize];
    let shape = edge_shape(solid, chain);
    let line = match shape {
        EdgeShape::Line { from, to } => [from, to - from],
        EdgeShape::Circle { centre, axis, .. } => [centre, axis],
        EdgeShape::Ellipse { .. } | EdgeShape::Other => {
            return Err(Failed::shown(
                why::EDGE_NOT_AN_AXIS,
                chain_curves(solid, chain, tolerance),
            ));
        }
    };
    match chain_runs_with(&topology, chain, edge) {
        Some(true) => Ok(line),
        Some(false) => Ok([line[0], -line[1]]),
        None => Err(Failed::shown(
            why::EDGE_UNDIRECTED,
            chain_curves(solid, chain, tolerance),
        )),
    }
}

/// The axis of the round face `face` names on `solid`, see
/// [`resolve_axis`]: the kernel's ([`Topology::face_axis`]), through the
/// point of the axis nearest the face's point, not the form's own point:
/// a cone that's nearly a cylinder has its apex far along its axis, past
/// where the workers' bytes take a draft's axis, and the axis is drawn
/// from its point. The face's point is on the face, within `MAX_COORD`,
/// so the one found is within the wire's bounds even where the face is
/// at the limit.
///
/// [`Topology::face_axis`]: varde_kernel_compat::Topology::face_axis
fn face_axis(solid: &Solid, face: &FaceRef, tolerance: &Tolerance) -> Result<[DVec3; 2], Failed> {
    let topology = solid.topology();
    topology
        .face_axis(solid, &face.key, face.near)
        .map_err(|why| match why {
            Unresolved::NotRound { region } => Failed::shown(
                why::AXIS_FACE_NOT_ROUND,
                face_geometry(solid, &topology.regions()[region as usize], tolerance),
            ),
            _ => why::Picked::AxisFace.not_found().into(),
        })
}

/// The plane of the flat face `face` names on `solid`, its unit normal
/// out of the solid and offset, `n·x = d`, or why there's none, worded
/// by `words`. [`resolve_plane`] takes `n·d` for its point (`n` is unit,
/// so a face square to a world axis gives its coordinate to the bit).
pub(super) fn face_plane(
    solid: &Solid,
    face: &FaceRef,
    words: &PlaneWords,
    tolerance: &Tolerance,
) -> Result<(DVec3, f64), Failed> {
    let topology = solid.topology();
    let region =
        (topology.face(solid, &face.key, face.near)).map_err(|_| words.picked.not_found())?;
    let region = &topology.regions()[region as usize];
    match *region_form(solid, region) {
        Form::Plane { n, d } if n != DVec3::ZERO && n.is_finite() && d.is_finite() => Ok((n, d)),
        _ => Err(Failed::shown(
            words.picked.not_flat(),
            face_geometry(solid, region, tolerance),
        )),
    }
}

//! Evaluating a move or a mirror: the bodies' solids, as the features
//! before it leave them, moved by one [`Motion`] each feature works out
//! from its values and the axis or plane it names.

use glam::DVec3;
use std::f64::consts::PI;
use varde_document::{
    AxisRef, BodyId, Document, EdgeRef, FaceRef, FeatureId, MAX_COORD, Mirror, Move, Plane,
};
use varde_kernel_compat::measure::{EdgeShape, edge_shape};
use varde_kernel_compat::mesh::Form;
use varde_kernel_compat::topology::Unresolved;
use varde_kernel_compat::{Budget, Instance, Motion, Solid, Tolerance, assemble};

use super::{
    BodySolid, Evaluation, Failed, chain_curves, chain_runs_with, face_geometry, own_solids,
};
use crate::cache::{Cache, Key, Keyer};
use crate::error_geometry::KernelFailure;
use crate::picking::region_form;
use crate::why::{self, Moving};

/// Changes the bodies of `evaluation` (those the features before it
/// made) as the move `moved` says, or says why it
/// fails, changing nothing.
///
/// Every body it names must have a solid of its own, as a combine's
/// ([`own_solids`]). Its motion is the turn about its axis
/// ([`resolve_axis`], on the bodies as the features before the move
/// leave them) by its angle, then the shift by its offsets. The angle is
/// stored in radians and turned in degrees ([`Motion::turn`]): typed in
/// degrees, it comes back to the number typed, so quarter turns are
/// exact. Each body is then moved ([`moved`]), its faces keeping their
/// names.
pub(super) fn evaluate_move(
    document: &Document,
    feature: FeatureId,
    moved: &Move,
    tolerance: &Tolerance,
    evaluation: &mut Evaluation,
    cache: &mut Cache,
) -> Result<(), Failed> {
    own_solids(document, moved.bodies.iter().copied(), evaluation)?;
    let turn = match &moved.turn {
        Some((axis, angle)) => {
            let [point, direction] = resolve_axis(axis, evaluation, tolerance, cache)?;
            note_reference(evaluation, feature, [point, direction]);
            let degrees = angle.value / (PI / 180.0);
            Motion::turn(point, direction, degrees).ok_or(why::AXIS_NO_DIRECTION)?
        }
        None => Motion::IDENTITY,
    };
    // Each offset is checked finite and within the coordinate limit.
    let shift = Motion::translation(moved.offset_vector()).ok_or(why::OFFSET_NOT_FINITE)?;
    let motion = turn.then(&shift);
    let how = How {
        moving: Moving::Move,
        copy: None,
    };
    place(
        document,
        &moved.bodies,
        &motion,
        how,
        tolerance,
        evaluation,
        cache,
    )
}

/// Changes the bodies of `evaluation` as the mirror `mirror`, the
/// feature `feature`, says, or says why it fails, changing nothing.
///
/// Its bodies must have solids of their own, as a move's. Its plane is an
/// origin plane or a flat face as the features before it leave its body
/// ([`resolve_plane`]). Without the original a body becomes its image,
/// its faces keeping their names as a move's do; with it, the body and
/// its image, whose faces are named as copy 1 of the mirror
/// ([`Instance`]), are put together by [`assemble`]: side by side where
/// they're apart, united where they meet (a body mirrored in its own
/// face).
pub(super) fn evaluate_mirror(
    document: &Document,
    feature: FeatureId,
    mirror: &Mirror,
    tolerance: &Tolerance,
    evaluation: &mut Evaluation,
    cache: &mut Cache,
) -> Result<(), Failed> {
    own_solids(document, mirror.bodies.iter().copied(), evaluation)?;
    let [point, normal] =
        resolve_plane(&mirror.plane, &MIRROR_PLANE, evaluation, tolerance, cache)?;
    note_reference(evaluation, feature, [point, normal]);
    let motion = Motion::mirror(point, normal).ok_or(why::MIRROR_FACE_NOT_FLAT)?;
    let how = How {
        moving: Moving::Mirror,
        copy: mirror.keep_original.then_some(Instance {
            feature: feature.get(),
            index: 1,
        }),
    };
    place(
        document,
        &mirror.bodies,
        &motion,
        how,
        tolerance,
        evaluation,
        cache,
    )
}

/// Notes `reference`, the axis or plane `feature` found, in
/// `evaluation` for its draft's reply, if the workers' wire takes it
/// ([`crate::reference_fits`]): one that's out of bounds there (a round
/// edge's centre far out, an axis of no length) isn't drawn, natively
/// as on the web.
pub(super) fn note_reference(
    evaluation: &mut Evaluation,
    feature: FeatureId,
    reference: [DVec3; 2],
) {
    if crate::reference_fits(&reference.map(|v| v.to_array())) {
        evaluation.references.push((feature, reference));
    }
}

/// How [`place`] moves a body.
#[derive(Clone, Copy)]
pub(super) struct How {
    /// What it's doing, for the messages.
    pub(super) moving: Moving,
    /// With the original kept, the copy the image is named as; `None`
    /// replaces the body by its image, keeping its faces' names.
    pub(super) copy: Option<Instance>,
}

/// Moves each of `bodies` (each with a solid of its own in
/// `evaluation`) by `motion`, as `how` says, or says why one can't be,
/// changing none. A body whose box `motion` takes past [`MAX_COORD`]
/// is refused before it's moved ("out of range": the box's image holds
/// the solid's, so one within the limit is too); then
/// [`Solid::transformed`] moves it, cached by the body's key, the
/// motion's bits ([`Motion::bits`]), the copy and the fit tolerance, so
/// an edit that leaves the motion as it was finds every body again.
pub(super) fn place(
    document: &Document,
    bodies: &[BodyId],
    motion: &Motion,
    how: How,
    tolerance: &Tolerance,
    evaluation: &mut Evaluation,
    cache: &mut Cache,
) -> Result<(), Failed> {
    let mut changed = Vec::with_capacity(bodies.len());
    for made in (evaluation.bodies.iter()).filter(|made| bodies.binary_search(&made.body).is_ok()) {
        let name = super::body_name(document, made.body);
        if !within(&made.solid, motion) {
            return Err(why::out_of_range(how.moving, name).into());
        }
        let key = moved_key(made.key, motion, how.copy, tolerance);
        let solid = cache.solid(key, || moved(&made.solid, motion, how, tolerance, name))?;
        changed.push(BodySolid {
            body: made.body,
            solid,
            key,
        });
    }
    for change in changed {
        evaluation.replace(change);
    }
    Ok(())
}

/// Whether `motion` keeps `solid` within [`MAX_COORD`]: every corner of
/// its box taken there is (the image of the box holds the solid's).
/// Every number is finite: the box is within the limit, and so are the
/// motion's offsets and its matrix's entries (at most 1 each, or a
/// scale's factors, at most 1e3).
pub(super) fn within(solid: &Solid, motion: &Motion) -> bool {
    let Some(bounds) = solid.bounds3() else {
        return true;
    };
    let max = f64::from(MAX_COORD);
    super::corners(&bounds).into_iter().all(|point| {
        let image = motion.point(point);
        image.is_finite() && image.abs().max_element() <= max
    })
}

/// The key of `motion` moving the solid filed under `body`, as `copy`
/// says, at `tolerance`.
fn moved_key(body: Key, motion: &Motion, copy: Option<Instance>, tolerance: &Tolerance) -> Key {
    let mut keyer = Keyer::new("moved");
    keyer.key(body).numbers(motion.bits());
    match copy {
        Some(Instance { feature, index }) => keyer.number(1).number(feature).number(index),
        None => keyer.number(0),
    };
    keyer.number(tolerance.fit().to_bits()).finish()
}

/// `solid`, the body named `name`'s, moved by `motion` as `how` says: its
/// image, or it and its image assembled; or why not, with the kernel's
/// evidence (patches and curves by value: the body is drawn where the
/// history leaves it, not where this went).
fn moved(
    solid: &Solid,
    motion: &Motion,
    how: How,
    tolerance: &Tolerance,
    name: &str,
) -> Result<Solid, Failed> {
    let kernel = |words: String, failure| {
        let failure = KernelFailure::new(failure, tolerance);
        Failed::kernel(words, &failure, [&[], &[]])
    };
    let image = solid
        .transformed(motion, how.copy, tolerance, &Budget::DEFAULT)
        .map_err(|failure| kernel(why::moving(how.moving, name, failure.error), failure))?;
    if how.copy.is_none() {
        return Ok(image);
    }
    let both = [solid.clone(), image];
    assemble(&both, tolerance, &Budget::DEFAULT)
        .map_err(|failure| kernel(why::with_image(name, failure.error), failure))
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
            let made = evaluation.holding(edge.body).ok_or(why::EDGE_BODY_GONE)?;
            let key = reference_key("axis edge", made.key, &edge.faces, edge.near, tolerance);
            cache.reference(key, || edge_line(&made.solid, edge, tolerance))
        }
        AxisRef::Face(face) => {
            let made = evaluation
                .holding(face.body)
                .ok_or(why::AXIS_FACE_BODY_GONE)?;
            let key = reference_key("axis face", made.key, &face.key, face.near, tolerance);
            cache.reference(key, || face_axis(&made.solid, face, tolerance))
        }
    }
}

/// What a feature calls the plane it names, for its failures: the
/// cache key's kind, and why a face fails (its body gone, the face not
/// found, the face not flat).
pub(super) struct PlaneWords {
    pub(super) kind: &'static str,
    pub(super) body_gone: &'static str,
    not_found: &'static str,
    not_flat: &'static str,
}

/// A split's plane: "its plane face wasn't found".
pub(super) const SPLIT_PLANE: PlaneWords = PlaneWords {
    kind: "split plane face",
    body_gone: why::SPLIT_PLANE_BODY_GONE,
    not_found: why::SPLIT_PLANE_NOT_FOUND,
    not_flat: why::SPLIT_PLANE_NOT_FLAT,
};

/// A mirror's plane: "its mirror face wasn't found".
pub(super) const MIRROR_PLANE: PlaneWords = PlaneWords {
    kind: "mirror face",
    body_gone: why::MIRROR_FACE_BODY_GONE,
    not_found: why::MIRROR_FACE_NOT_FOUND,
    not_flat: why::MIRROR_FACE_NOT_FLAT,
};

/// A draft's neutral plane: "its neutral face wasn't found".
pub(super) const NEUTRAL_PLANE: PlaneWords = PlaneWords {
    kind: "neutral face",
    body_gone: why::NEUTRAL_FACE_BODY_GONE,
    not_found: why::NEUTRAL_FACE_NOT_FOUND,
    not_flat: why::NEUTRAL_FACE_NOT_FLAT,
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
            let made = evaluation.holding(face.body).ok_or(words.body_gone)?;
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
    let chain = (topology.edge(solid, edge.faces, edge.near)).map_err(|_| why::EDGE_NOT_FOUND)?;
    let chain = &topology.chains()[chain as usize];
    let shape = edge_shape(solid, chain);
    let line = match shape {
        EdgeShape::Line { from, to } => [from, to - from],
        EdgeShape::Circle { centre, axis, .. } => [centre, axis],
        EdgeShape::Ellipse { .. } | EdgeShape::Other => {
            return Err(Failed {
                missing: false,
                message: why::EDGE_NOT_AN_AXIS.to_owned(),
                geometry: chain_curves(solid, chain, tolerance),
            });
        }
    };
    match chain_runs_with(&topology, chain, edge) {
        Some(true) => Ok(line),
        Some(false) => Ok([line[0], -line[1]]),
        None => Err(Failed {
            missing: false,
            message: why::EDGE_UNDIRECTED.to_owned(),
            geometry: chain_curves(solid, chain, tolerance),
        }),
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
            Unresolved::NotRound { region } => Failed {
                missing: false,
                message: why::AXIS_FACE_NOT_ROUND.to_owned(),
                geometry: face_geometry(solid, &topology.regions()[region as usize], tolerance),
            },
            _ => why::AXIS_FACE_NOT_FOUND.into(),
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
    let region = (topology.face(solid, &face.key, face.near)).map_err(|_| words.not_found)?;
    let region = &topology.regions()[region as usize];
    match *region_form(solid, region) {
        Form::Plane { n, d } if n != DVec3::ZERO && n.is_finite() && d.is_finite() => Ok((n, d)),
        _ => Err(Failed {
            missing: false,
            message: words.not_flat.to_owned(),
            geometry: face_geometry(solid, region, tolerance),
        }),
    }
}

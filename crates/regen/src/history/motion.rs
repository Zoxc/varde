//! Evaluating a move or a mirror: the bodies' solids, as the features
//! before it leave them, moved by one [`Motion`] each feature works out
//! from its values and the axis or plane it names.

use glam::DVec3;
use std::f64::consts::PI;
use varde_document::{
    AxisRef, BodyId, Document, EdgeRef, FaceRef, FeatureId, MAX_COORD, Mirror, Move, PlaneRef,
};
use varde_kernel::measure::{EdgeShape, edge_shape};
use varde_kernel::mesh::Form;
use varde_kernel::{Budget, Instance, Motion, Solid, Tolerance, assemble};

use super::{
    BodySolid, Evaluation, Failed, chain_curves, chain_runs_with, face_geometry, own_solids,
};
use crate::cache::{Cache, Key, Keyer};
use crate::error_geometry::KernelFailure;
use crate::message::{self, Moving};
use crate::picking::region_form;

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
            evaluation.references.push((feature, [point, direction]));
            let degrees = angle.value / (PI / 180.0);
            Motion::turn(point, direction, degrees).ok_or(message::AXIS_NO_DIRECTION)?
        }
        None => Motion::IDENTITY,
    };
    // Each offset is checked finite and within the coordinate limit.
    let shift = Motion::translation(moved.offset_vector()).ok_or(message::OFFSET_NOT_FINITE)?;
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
    let [point, normal] = resolve_plane(&mirror.plane, evaluation, tolerance, cache)?;
    evaluation.references.push((feature, [point, normal]));
    let motion = Motion::mirror(point, normal).ok_or(message::MIRROR_FACE_NOT_FLAT)?;
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

/// How [`place`] moves a body.
#[derive(Clone, Copy)]
struct How {
    /// What it's doing, for the messages.
    moving: Moving,
    /// With the original kept, the copy the image is named as; `None`
    /// replaces the body by its image, keeping its faces' names.
    copy: Option<Instance>,
}

/// Moves each of `bodies` (each with a solid of its own in
/// `evaluation`) by `motion`, as `how` says, or says why one can't be,
/// changing none. A body whose box `motion` takes past [`MAX_COORD`]
/// is refused before it's moved ("out of range": the box's image holds
/// the solid's, so one within the limit is too); then
/// [`Solid::transformed`] moves it, cached by the body's key, the
/// motion's bits ([`Motion::bits`]), the copy and the fit tolerance, so
/// an edit that leaves the motion as it was finds every body again.
fn place(
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
        let name = document
            .body(made.body)
            .map_or("a body", |body| body.name.as_str());
        if !within(&made.solid, motion) {
            return Err(message::out_of_range(how.moving, name).into());
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
        if let Some(made) = evaluation.bodies.iter_mut().find(|m| m.body == change.body) {
            *made = change;
        }
    }
    Ok(())
}

/// Whether `motion` keeps `solid` within [`MAX_COORD`]: every corner of
/// its box taken there is (the image of the box holds the solid's).
/// Every number is finite: the box is within the limit, and so are the
/// motion's offsets and its matrix's entries (at most 1 each).
fn within(solid: &Solid, motion: &Motion) -> bool {
    let Some(bounds) = solid.bounds3() else {
        return true;
    };
    let max = f64::from(MAX_COORD);
    (0..8).all(|corner| {
        let pick = |bit: u32, min: f64, max: f64| if corner & bit == 0 { min } else { max };
        let point = DVec3::new(
            pick(1, bounds.min.x, bounds.max.x),
            pick(2, bounds.min.y, bounds.max.y),
            pick(4, bounds.min.z, bounds.max.z),
        );
        let image = motion.point(point);
        image.is_finite() && image.abs().max_element() <= max
    })
}

/// The key of `motion` moving the solid filed under `body`, as `copy`
/// says, at `tolerance`.
fn moved_key(body: Key, motion: &Motion, copy: Option<Instance>, tolerance: &Tolerance) -> Key {
    let mut keyer = Keyer::new("moved");
    keyer.key(body);
    for bits in motion.bits() {
        keyer.number(bits);
    }
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
        .map_err(|failure| kernel(message::moving(how.moving, name, failure.error), failure))?;
    if how.copy.is_none() {
        return Ok(image);
    }
    let both = [solid.clone(), image];
    assemble(&both, tolerance, &Budget::DEFAULT)
        .map_err(|failure| kernel(message::with_image(name, failure.error), failure))
}

/// The line `axis` names, as a point on it and its direction (not unit,
/// not zero), on the bodies as the features before the one naming it
/// leave them (`evaluation`), or why there's none. An origin axis is
/// through the origin. An edge or a face is looked for on its body
/// through [`Evaluation::holder`] (a body with no solid is "gone") and
/// found on its solid's topology by its names and point, as a revolve's
/// axis edge is ([`super::edge_axis`]): an edge must be straight (the
/// line through it, directed as [`EdgeRef`] says) or round (its circle's
/// axis through its centre, turning the way the edge runs as so
/// directed); a face must be round, a cylinder, cone, torus or other
/// surface of revolution (its form's axis, directed as the form has it,
/// through the point of it nearest the face's point: [`beside`]).
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
            let made = holding(edge.body, evaluation).ok_or(message::EDGE_BODY_GONE)?;
            let key = reference_key("axis edge", made.key, &edge.faces, edge.near, tolerance);
            cache.reference(key, || edge_line(&made.solid, edge, tolerance))
        }
        AxisRef::Face(face) => {
            let made = holding(face.body, evaluation).ok_or(message::AXIS_FACE_BODY_GONE)?;
            let key = reference_key("axis face", made.key, &face.key, face.near, tolerance);
            cache.reference(key, || face_axis(&made.solid, face, tolerance))
        }
    }
}

/// The plane `plane` names, as a point on it and its normal (not unit,
/// not zero), as [`resolve_axis`] finds a line: an origin plane through
/// the origin; a face must be flat (its form's plane, its normal out of
/// the solid).
pub(super) fn resolve_plane(
    plane: &PlaneRef,
    evaluation: &Evaluation,
    tolerance: &Tolerance,
    cache: &mut Cache,
) -> Result<[DVec3; 2], Failed> {
    match plane {
        PlaneRef::Origin(origin) => Ok([DVec3::ZERO, origin.placement().normal]),
        PlaneRef::Face(face) => {
            let made = holding(face.body, evaluation).ok_or(message::MIRROR_FACE_BODY_GONE)?;
            let key = reference_key("mirror face", made.key, &face.key, face.near, tolerance);
            cache.reference(key, || face_plane(&made.solid, face, tolerance))
        }
    }
}

/// The body in `evaluation` holding `body`'s solid, if it has one.
pub(super) fn holding(body: BodyId, evaluation: &Evaluation) -> Option<&BodySolid> {
    let holder = evaluation.holder(body)?;
    evaluation.bodies.iter().find(|made| made.body == holder)
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
    keyer.key(solid).value(names);
    for number in near.to_array() {
        keyer.number(number.to_bits());
    }
    keyer.number(tolerance.fit().to_bits()).finish()
}

/// The line through the edge `edge` names on `solid`, or its circle's
/// axis, see [`resolve_axis`].
fn edge_line(solid: &Solid, edge: &EdgeRef, tolerance: &Tolerance) -> Result<[DVec3; 2], Failed> {
    let topology = solid.topology();
    let chain =
        (topology.edge(solid, edge.faces, edge.near)).map_err(|_| message::EDGE_NOT_FOUND)?;
    let chain = &topology.chains()[chain as usize];
    let shape = edge_shape(solid, chain);
    let line = match shape {
        EdgeShape::Line { from, to } => [from, to - from],
        EdgeShape::Circle { centre, axis, .. } => [centre, axis],
        EdgeShape::Ellipse { .. } | EdgeShape::Other => {
            return Err(Failed {
                message: message::EDGE_NOT_AN_AXIS.to_owned(),
                geometry: chain_curves(solid, chain, tolerance),
            });
        }
    };
    match chain_runs_with(&topology, chain, edge) {
        Some(true) => Ok(line),
        Some(false) => Ok([line[0], -line[1]]),
        None => Err(Failed {
            message: message::EDGE_UNDIRECTED.to_owned(),
            geometry: chain_curves(solid, chain, tolerance),
        }),
    }
}

/// The axis of the round face `face` names on `solid`, see
/// [`resolve_axis`].
fn face_axis(solid: &Solid, face: &FaceRef, tolerance: &Tolerance) -> Result<[DVec3; 2], Failed> {
    let topology = solid.topology();
    let region =
        (topology.face(solid, &face.key, face.near)).map_err(|_| message::AXIS_FACE_NOT_FOUND)?;
    let region = &topology.regions()[region as usize];
    let [point, axis] = match *region_form(solid, region) {
        Form::Cylinder { point, axis, .. } => [point, axis],
        Form::Cone { apex, axis, .. } => [apex, axis],
        Form::Torus { centre, axis, .. } => [centre, axis],
        Form::Revolved { origin, axis, .. } => [origin, axis],
        _ => {
            return Err(Failed {
                message: message::AXIS_FACE_NOT_ROUND.to_owned(),
                geometry: face_geometry(solid, region, tolerance),
            });
        }
    };
    Ok([beside(point, axis, face.near), axis])
}

/// The point of the line through `point` along `axis` nearest `near`, or
/// `point` where that isn't a finite point within [`MAX_COORD`] (an axis
/// of no length). A form's own point can be far out: a cone that's
/// nearly a cylinder has its apex far along its axis, past where the
/// workers' bytes take a draft's axis, and the axis is drawn from its
/// point. The face's point is on the face, within the limit, so the
/// point found is beside the face.
pub(super) fn beside(point: DVec3, axis: DVec3, near: DVec3) -> DVec3 {
    let foot = point + axis * ((near - point).dot(axis) / axis.length_squared());
    let max = f64::from(MAX_COORD);
    if foot.is_finite() && foot.abs().max_element() <= max {
        foot
    } else {
        point
    }
}

/// The plane of the flat face `face` names on `solid`, see
/// [`resolve_plane`]: the point of `n·x = d` nearest the origin, `n·d`
/// (`n` is unit, so a face square to a world axis gives its coordinate
/// to the bit), and `n`.
fn face_plane(solid: &Solid, face: &FaceRef, tolerance: &Tolerance) -> Result<[DVec3; 2], Failed> {
    let topology = solid.topology();
    let region =
        (topology.face(solid, &face.key, face.near)).map_err(|_| message::MIRROR_FACE_NOT_FOUND)?;
    let region = &topology.regions()[region as usize];
    match *region_form(solid, region) {
        Form::Plane { n, d } if n != DVec3::ZERO && n.is_finite() && d.is_finite() => {
            Ok([n * d, n])
        }
        _ => Err(Failed {
            message: message::MIRROR_FACE_NOT_FLAT.to_owned(),
            geometry: face_geometry(solid, region, tolerance),
        }),
    }
}

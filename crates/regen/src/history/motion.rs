//! Evaluating a move or a mirror: the bodies' solids, as the features
//! before it leave them, moved by one [`Motion`] each feature works out
//! from its values and the axis or plane it names.

use std::f64::consts::PI;
use varde_document::{BodyId, Document, MAX_COORD, Mirror, Move};
use varde_kernel_compat::{Budget, Instance, Motion, Solid, Tolerance, assemble};

use super::reference::{MIRROR_PLANE, note_reference, resolve_axis, resolve_plane};
use super::{BodySolid, Evaluation, Failed, Step, own_solids};
use crate::cache::{Cache, Key, Keyer};
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
    step: &Step,
    moved: &Move,
    evaluation: &mut Evaluation,
    cache: &mut Cache,
) -> Result<(), Failed> {
    let Step {
        document,
        feature,
        tolerance,
        ..
    } = *step;
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
    step: &Step,
    mirror: &Mirror,
    evaluation: &mut Evaluation,
    cache: &mut Cache,
) -> Result<(), Failed> {
    let Step {
        document,
        feature,
        tolerance,
        ..
    } = *step;
    own_solids(document, mirror.bodies.iter().copied(), evaluation)?;
    let [point, normal] =
        resolve_plane(&mirror.plane, &MIRROR_PLANE, evaluation, tolerance, cache)?;
    note_reference(evaluation, feature, [point, normal]);
    let motion = Motion::mirror(point, normal).ok_or(why::Picked::MirrorFace.not_flat())?;
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
    let kernel = |words: String, failure| Failed::by_value(words, failure, tolerance);
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

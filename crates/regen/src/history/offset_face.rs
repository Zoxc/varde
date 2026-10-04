//! Evaluating an offset face: the body's solid, as the features before
//! it leave it, with the faces it names moved along their normals by
//! the kernel's [`varde_kernel::offset_faces()`].
//!
//! The faces are found on the body's topology (the one drawing it
//! keeps) by their keys and points, as a shell's
//! open faces are; one not found fails the offset before the kernel
//! ("its face wasn't found", "its face 2 of 3 wasn't found"). The
//! regions found go to the kernel sorted, each once (two references may
//! name one face), with the distance signed (negative inward) and
//! whether tangent faces are taken in, which the kernel grows.
//!
//! The result is cached by the body's key, the regions, the signed
//! distance's bits, the tangent flag, the feature and the fit
//! tolerance, and replaces the body's solid (the body keeps its id and
//! every face its name, so a sketch on a moved face follows it). The
//! kernel's refusals are worded for the Timeline
//! (`message::offset_refused`), with the face or corner they're about
//! drawn; its other failures as a boolean's ("offsetting faces of Body
//! 1 ...").
//!
//! The kernel's offset face isn't built yet: it fails as too complex,
//! which reaches the user as "offsetting faces of Body 1 is too complex
//! to work out", and the rest of the history goes on.

use varde_document::{Document, FeatureId, OffsetFace};
use varde_kernel::{Budget, Evidence, OffsetError, Solid, Tolerance, Topology};

use super::in_place::{self, InPlace};
use super::{Evaluation, Failed};
use crate::ErrorGeometry;
use crate::cache::Cache;
use crate::error_geometry::KernelFailure;
use crate::message;

/// The kernel's offset face, which tests may replace with a stand-in to
/// check what regeneration does with the result before the kernel's is
/// built.
type Offsetter = fn(
    &Solid,
    &Topology,
    &[u32],
    f64,
    bool,
    u64,
    &Tolerance,
    &Budget,
) -> Result<Solid, OffsetError>;

#[cfg(any(test, feature = "testing"))]
thread_local! {
    /// The offset a test asks for in place of the kernel's, on its own
    /// thread.
    pub(crate) static OFFSETTER: std::cell::Cell<Option<Offsetter>> =
        const { std::cell::Cell::new(None) };
}

/// The offset to run: the kernel's, or the one a test set.
fn offsetter() -> Offsetter {
    #[cfg(any(test, feature = "testing"))]
    if let Some(offsetter) = OFFSETTER.get() {
        return offsetter;
    }
    varde_kernel::offset_faces
}

/// Offsets faces on this thread by [`by_boxes`] from now on.
#[cfg(any(test, feature = "testing"))]
pub(crate) fn offset_by_boxes() {
    OFFSETTER.set(Some(by_boxes));
}

/// A stand-in for the kernel's offset face, for tests: the solid must be
/// a box along the world's axes ([`in_place::box_sides`]), and each face
/// picked moves along its normal by `distance`, the four around it
/// stretched to meet it: one scale along each axis and a move, so every
/// face keeps its name (as the kernel's keeps them). A face moved onto
/// or past the face opposite it (the walls between shrinking to
/// nothing) is [`OffsetError::PastNeighbour`], a box past
/// [`MAX_COORD`](varde_kernel::MAX_COORD) [`OffsetError::OutOfRange`];
/// anything else is [`KernelError::TooComplex`].
///
/// [`KernelError::TooComplex`]: varde_kernel::KernelError::TooComplex
#[cfg(any(test, feature = "testing"))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn by_boxes(
    solid: &Solid,
    topology: &Topology,
    faces: &[u32],
    distance: f64,
    _tangent: bool,
    _feature: u64,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<Solid, OffsetError> {
    use varde_kernel::KernelError;
    use varde_kernel::Motion;
    let too_complex = || OffsetError::Failed(KernelError::TooComplex.into());
    let (min, max, sides) = in_place::box_sides(solid, topology).ok_or_else(too_complex)?;
    let size = max - min;
    let (mut lo, mut hi) = (min, max);
    for &region in faces {
        let (axis, end) = *sides.get(region as usize).ok_or_else(too_complex)?;
        if end == 0 {
            lo[axis] = min[axis] - distance;
        } else {
            hi[axis] = max[axis] + distance;
        }
    }
    // The walls between a face and the one opposite it shrink to
    // nothing (the resolution is the least they may keep).
    let least = tol.resolution();
    for axis in 0..3 {
        if hi[axis] - lo[axis] <= least {
            let moved = (faces.iter())
                .copied()
                .find(|&region| sides[region as usize].0 == axis)
                .expect("only a moved face shrinks an axis");
            return Err(OffsetError::PastNeighbour { region: moved });
        }
    }
    let reach = f64::from(varde_kernel::MAX_COORD);
    if lo.abs().max_element() > reach || hi.abs().max_element() > reach {
        return Err(OffsetError::OutOfRange);
    }
    let factors = (hi - lo) / size;
    let motion = Motion::scale(min, factors)
        .zip(Motion::translation(lo - min))
        .map(|(scale, shift)| scale.then(&shift))
        .ok_or_else(too_complex)?;
    debug_assert!((motion.point(max) - hi).abs().max_element() <= 1e-9 * (1.0 + size.length()));
    Ok(solid.transformed(&motion, None, tol, budget)?)
}

/// Changes the body of `evaluation` as the offset face `offset`, the
/// feature `feature`, says, or says why it fails, changing nothing.
///
/// Its body must have a solid of its own ([`InPlace::of`]). Then the
/// faces and the kernel's offset, as the module's docs say.
pub(super) fn evaluate_offset_face(
    document: &Document,
    feature: FeatureId,
    offset: &OffsetFace,
    tolerance: &Tolerance,
    evaluation: &mut Evaluation,
    cache: &mut Cache,
) -> Result<(), Failed> {
    let body = offset.body().expect("a checked offset face has faces");
    let place = InPlace::of(document, body, evaluation, cache)?;
    let faces = place.regions(&offset.faces, message::offset_face_not_found)?;
    let distance = offset.signed_distance();
    let mut keyer = place.keyer("offset face", feature, tolerance);
    keyer
        .number(distance.to_bits())
        .number(u64::from(offset.tangent))
        .number(faces.len() as u64);
    for &region in &faces {
        keyer.number(u64::from(region));
    }
    let key = keyer.finish();
    let offset_by = offsetter();
    place.replace(
        key,
        evaluation,
        cache,
        message::offset_leaves_nothing,
        |budget| {
            offset_by(
                &place.solid,
                &place.topology,
                &faces,
                distance,
                offset.tangent,
                feature.get(),
                tolerance,
                budget,
            )
            .map_err(|error| refused(error, &place, tolerance))
        },
    )
}

/// Why the kernel's offset of faces of the body `place` gave no solid,
/// in words, with what to draw: the face or corner a refusal is about,
/// or the failure's evidence.
fn refused(error: OffsetError, place: &InPlace<'_>, tolerance: &Tolerance) -> Failed {
    let (solid, topology) = (&*place.solid, &*place.topology);
    let mut evidence = Evidence::default();
    let mut region_drawn =
        |region: u32| in_place::draw_region(&mut evidence, solid, topology, region);
    let why = match error {
        OffsetError::PastNeighbour { region } => {
            region_drawn(region);
            message::OffsetRefusal::PastNeighbour
        }
        OffsetError::IntoBody => message::OffsetRefusal::IntoBody,
        OffsetError::RoundTooSmall { region } => {
            region_drawn(region);
            message::OffsetRefusal::RoundTooSmall
        }
        OffsetError::NoSurface { region } => {
            region_drawn(region);
            message::OffsetRefusal::NoSurface
        }
        OffsetError::TangentNeighbour { region } => {
            region_drawn(region);
            message::OffsetRefusal::TangentNeighbour
        }
        OffsetError::Corner { vertex } => {
            evidence.add_points(solid.mesh().verts().get(vertex as usize).copied());
            message::OffsetRefusal::Corner
        }
        OffsetError::OutOfRange => message::OffsetRefusal::OutOfRange,
        OffsetError::Failed(failure) => {
            let words = message::offsetting(place.name, failure.error);
            let failure = KernelFailure::new(failure, tolerance);
            return Failed::kernel(words, &failure, [&[place.body], &[]]);
        }
    };
    Failed {
        message: message::offset_refused(why, place.name),
        geometry: ErrorGeometry::of_evidence(&evidence, tolerance),
    }
}

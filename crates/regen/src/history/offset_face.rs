//! Evaluating an offset face: the body's solid, as the features before
//! it leave it, with the faces it names moved along their normals by
//! the kernel's [`varde_kernel_compat::offset_faces()`].
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
//! (`why::offset_refused`), with the face or corner they're about
//! drawn; its other failures as a boolean's ("offsetting faces of Body
//! 1 ...").
//!
//! The kernel's offset face isn't built yet: it fails as too complex,
//! which reaches the user as "offsetting faces of Body 1 is too complex
//! to work out", and the rest of the history goes on.

use varde_document::OffsetFace;
use varde_kernel_compat::{Budget, OffsetError, Solid, Tolerance, Topology};

use super::in_place::{Drawn, InPlace};
use super::{Evaluation, Failed, Step};
use crate::cache::Cache;
use crate::why;

/// The kernel's offset face, which tests may replace with a stand-in to
/// check what regeneration does with the result before the kernel's is
/// built.
pub(crate) type Offsetter = fn(
    &Solid,
    &Topology,
    &[u32],
    f64,
    bool,
    u64,
    &Tolerance,
    &Budget,
) -> Result<Solid, OffsetError>;

kernel_op! {
    /// The offset to run: the kernel's, or the one a test set.
    fn offsetter() -> Offsetter = varde_kernel_compat::offset_faces, offset;
}

/// Changes the body of `evaluation` as the offset face `offset`, the
/// feature `feature`, says, or says why it fails, changing nothing.
///
/// Its body must have a solid of its own ([`InPlace::of`]). Then the
/// faces and the kernel's offset, as the module's docs say.
pub(super) fn evaluate_offset_face(
    step: &Step,
    offset: &OffsetFace,
    evaluation: &mut Evaluation,
    cache: &mut Cache,
) -> Result<(), Failed> {
    let Step {
        document,
        feature,
        tolerance,
        ..
    } = *step;
    let body = offset.body().ok_or(why::NOTHING_PICKED)?;
    let place = InPlace::of(document, body, evaluation, cache)?;
    let faces = place.regions(&offset.faces, why::face_not_found)?;
    let distance = offset.signed_distance();
    let mut keyer = place.keyer("offset face", feature, tolerance);
    keyer
        .number(distance.to_bits())
        .number(u64::from(offset.tangent))
        .regions(&faces);
    let key = keyer.finish();
    let offset_by = offsetter();
    place.replace(
        key,
        evaluation,
        cache,
        why::offset_leaves_nothing,
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
    let (why, drawn) = match error {
        OffsetError::PastNeighbour { region } => {
            (why::OffsetRefusal::PastNeighbour, Drawn::Region(region))
        }
        OffsetError::IntoBody => (why::OffsetRefusal::IntoBody, Drawn::Nothing),
        OffsetError::RoundTooSmall { region } => {
            (why::OffsetRefusal::RoundTooSmall, Drawn::Region(region))
        }
        OffsetError::NoSurface { region } => (why::OffsetRefusal::NoSurface, Drawn::Region(region)),
        OffsetError::TangentNeighbour { region } => {
            (why::OffsetRefusal::TangentNeighbour, Drawn::Region(region))
        }
        OffsetError::Corner { vertex } => (why::OffsetRefusal::Corner, Drawn::Corner(vertex)),
        OffsetError::OutOfRange => (why::OffsetRefusal::OutOfRange, Drawn::Nothing),
        OffsetError::Failed(failure) => {
            let words = why::offsetting(place.name, failure.error);
            return place.kernel_failed(words, failure, tolerance);
        }
    };
    place.refused(why::offset_refused(why, place.name), drawn, tolerance)
}

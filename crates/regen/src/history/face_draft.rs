//! Evaluating a draft: the body's solid, as the features before it
//! leave it, with the faces it names turned about the neutral plane by
//! the kernel's [`varde_kernel::draft_faces()`].
//!
//! The faces are found on the body's topology (the one drawing it
//! keeps) by their keys and points, as an offset face's are; one not
//! found fails the draft before the kernel ("its face wasn't found",
//! "its face 2 of 3 wasn't found"). The neutral plane is an origin
//! plane, or a flat face found on its body as the features before the
//! draft leave it, as a mirror's plane is (its own failures: "its
//! neutral face wasn't found", "... isn't flat", "... body is gone").
//! The pull is the plane's normal (a face's outward normal, an origin
//! plane's axis), reversed by the draft's flip. The regions found go to
//! the kernel sorted, each once, with a point of the neutral plane, the
//! pull, the angle in radians and whether tangent faces are taken in.
//! The plane found is noted for the answer (`Evaluation::references`,
//! as a mirror's plane), its normal as found (not flipped), also when a
//! face isn't found (that failure still comes first), so the session
//! draws the plane while its faces are picked again.
//!
//! The result is cached by the body's key, the regions, the plane's
//! point and the pull's bits, the angle's bits, the tangent flag, the
//! feature and the fit tolerance, and replaces the body's solid (the
//! body keeps its id and every face its name, so a sketch on a drafted
//! face follows it). The kernel's refusals are worded for the Timeline
//! (`message::draft_refused`), with the face or corner they're about
//! drawn; its other failures as a boolean's ("drafting faces of Body 1
//! ...").
//!
//! The kernel's draft isn't built yet: it fails as too complex, which
//! reaches the user as "drafting faces of Body 1 is too complex to work
//! out", and the rest of the history goes on.

use glam::DVec3;
use varde_document::{Document, FaceDraft, FeatureId};
use varde_kernel::{Budget, DraftError, Evidence, Solid, Tolerance, Topology};

use super::in_place::{self, InPlace};
use super::motion::{NEUTRAL_PLANE, resolve_plane};
use super::{Evaluation, Failed};
use crate::ErrorGeometry;
use crate::cache::Cache;
use crate::error_geometry::KernelFailure;
use crate::message::{self, DraftRefusal};

#[cfg(any(test, feature = "testing"))]
pub(crate) mod stand_in;

/// The kernel's draft, which tests may replace with a stand-in to check
/// what regeneration does with the result before the kernel's is built.
pub(crate) type Drafter = fn(
    &Solid,
    &Topology,
    &[u32],
    DVec3,
    DVec3,
    f64,
    bool,
    u64,
    &Tolerance,
    &Budget,
) -> Result<Solid, DraftError>;

kernel_op! {
    /// The draft to run: the kernel's, or the one a test set.
    fn drafter() -> Drafter = varde_kernel::draft_faces, draft;
}

/// Changes the body of `evaluation` as the draft `draft`, the feature
/// `feature`, says, or says why it fails, changing nothing.
///
/// Its body must have a solid of its own ([`InPlace::of`]). Then the
/// faces, the neutral plane and the kernel's draft, as the module's docs
/// say.
pub(super) fn evaluate_face_draft(
    document: &Document,
    feature: FeatureId,
    draft: &FaceDraft,
    tolerance: &Tolerance,
    evaluation: &mut Evaluation,
    cache: &mut Cache,
) -> Result<(), Failed> {
    let body = draft.body().expect("a checked draft has faces");
    let place = InPlace::of(document, body, evaluation, cache)?;
    let faces = place.regions(&draft.faces, message::draft_face_not_found);
    let plane = resolve_plane(&draft.neutral, &NEUTRAL_PLANE, evaluation, tolerance, cache);
    // The plane, as a mirror's is, for the session to draw: also while a
    // face isn't found, whose failure comes first.
    if let Ok(&plane) = plane.as_ref() {
        super::motion::note_reference(evaluation, feature, plane);
    }
    let faces = faces?;
    let [neutral, normal] = plane?;
    let normal = normal.normalize();
    let pull = if draft.flip { -normal } else { normal };
    let angle = draft.angle.value;
    let mut keyer = place.keyer("face draft", feature, tolerance);
    for number in neutral.to_array().into_iter().chain(pull.to_array()) {
        keyer.number(number.to_bits());
    }
    keyer
        .number(angle.to_bits())
        .number(u64::from(draft.tangent))
        .number(faces.len() as u64);
    for &region in &faces {
        keyer.number(u64::from(region));
    }
    let key = keyer.finish();
    let draft_by = drafter();
    place.replace(
        key,
        evaluation,
        cache,
        message::draft_leaves_nothing,
        |budget| {
            draft_by(
                &place.solid,
                &place.topology,
                &faces,
                neutral,
                pull,
                angle,
                draft.tangent,
                feature.get(),
                tolerance,
                budget,
            )
            .map_err(|error| refused(error, &place, tolerance))
        },
    )
}

/// Why the kernel's draft of faces of the body `place` gave no solid,
/// in words, with what to draw: the face or corner a refusal is about,
/// or the failure's evidence.
fn refused(error: DraftError, place: &InPlace<'_>, tolerance: &Tolerance) -> Failed {
    let (solid, topology) = (&*place.solid, &*place.topology);
    let mut evidence = Evidence::default();
    let mut region_drawn =
        |region: u32| in_place::draw_region(&mut evidence, solid, topology, region);
    let why = match error {
        DraftError::FacingPull { region } => {
            region_drawn(region);
            DraftRefusal::FacingPull
        }
        DraftError::CannotDraft { region } => {
            region_drawn(region);
            DraftRefusal::CannotDraft
        }
        DraftError::PastNeighbour { region } => {
            region_drawn(region);
            DraftRefusal::PastNeighbour
        }
        DraftError::IntoBody => DraftRefusal::IntoBody,
        DraftError::RoundTooSmall { region } => {
            region_drawn(region);
            DraftRefusal::RoundTooSmall
        }
        DraftError::NoSurface { region } => {
            region_drawn(region);
            DraftRefusal::NoSurface
        }
        DraftError::TangentNeighbour { region } => {
            region_drawn(region);
            DraftRefusal::TangentNeighbour
        }
        DraftError::Corner { vertex } => {
            evidence.add_points(solid.mesh().verts().get(vertex as usize).copied());
            DraftRefusal::Corner
        }
        DraftError::OutOfRange => DraftRefusal::OutOfRange,
        DraftError::Failed(failure) => {
            let words = message::drafting(place.name, failure.error);
            let failure = KernelFailure::new(failure, tolerance);
            return Failed::kernel(words, &failure, [&[place.body], &[]]);
        }
    };
    Failed {
        message: message::draft_refused(why, place.name),
        geometry: ErrorGeometry::of_evidence(&evidence, tolerance),
    }
}

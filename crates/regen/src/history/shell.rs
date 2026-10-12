//! Evaluating a shell: the body's solid, as the features before it
//! leave it, hollowed by the kernel's [`varde_kernel::shell()`] and opened
//! through the faces it names.
//!
//! The open faces are found on the body's topology (the one drawing it
//! keeps) by their keys and points, as a split's
//! tool face is; one not found fails the shell before the kernel ("its
//! open face wasn't found", "its open face 2 of 3 wasn't found"). The
//! regions found go to the kernel sorted, each once (two references may
//! name one face).
//!
//! The result is cached by the body's key, the regions, the thickness's
//! bits, the direction, the feature and the fit tolerance, and replaces
//! the body's solid (the body keeps its id). The kernel's refusals are
//! worded for the Timeline (`message::shell_refused`), with the face or
//! corner they're about drawn; its other failures as a boolean's
//! ("shelling Body 1 ...").
//!
//! The kernel's shell isn't built yet: it fails as too complex, which
//! reaches the user as "shelling Body 1 is too complex to work out", and
//! the rest of the history goes on.

use varde_document::{Document, FeatureId, Shell};
use varde_kernel::{Budget, Evidence, ShellError, Solid, Tolerance, Topology};

use super::in_place::{self, InPlace};
use super::{Evaluation, Failed};
use crate::ErrorGeometry;
use crate::cache::Cache;
use crate::error_geometry::KernelFailure;
use crate::message;

#[cfg(any(test, feature = "testing"))]
pub(crate) mod stand_in;

/// The kernel's shell, which tests may replace with a stand-in to check
/// what regeneration does with the result before the kernel's is built.
pub(crate) type Sheller =
    fn(&Solid, &Topology, &[u32], f64, bool, u64, &Tolerance, &Budget) -> Result<Solid, ShellError>;

kernel_op! {
    /// The shell to run: the kernel's, or the one a test set.
    fn sheller() -> Sheller = varde_kernel::shell, shell;
}

/// Changes the body of `evaluation` as the shell `shell`, the feature
/// `feature`, says, or says why it fails, changing nothing.
///
/// Its body must have a solid of its own ([`InPlace::of`]). Then the
/// faces and the kernel's shell, as the module's docs say.
pub(super) fn evaluate_shell(
    document: &Document,
    feature: FeatureId,
    shell: &Shell,
    tolerance: &Tolerance,
    evaluation: &mut Evaluation,
    cache: &mut Cache,
) -> Result<(), Failed> {
    let place = InPlace::of(document, shell.body, evaluation, cache)?;
    let open = place.regions(&shell.open, message::shell_face_not_found)?;
    let thickness = shell.thickness.value;
    let mut keyer = place.keyer("shell", feature, tolerance);
    keyer
        .number(thickness.to_bits())
        .number(u64::from(shell.outward))
        .number(open.len() as u64);
    for &region in &open {
        keyer.number(u64::from(region));
    }
    let key = keyer.finish();
    let shell_by = sheller();
    place.replace(
        key,
        evaluation,
        cache,
        message::shell_leaves_nothing,
        |budget| {
            shell_by(
                &place.solid,
                &place.topology,
                &open,
                thickness,
                shell.outward,
                feature.get(),
                tolerance,
                budget,
            )
            .map_err(|error| refused(error, &place, tolerance))
        },
    )
}

/// Why the kernel's shell of the body `place` gave no solid, in words,
/// with what to draw: the face or corner a refusal is about, or the
/// failure's evidence.
fn refused(error: ShellError, place: &InPlace<'_>, tolerance: &Tolerance) -> Failed {
    let solid = &*place.solid;
    let mut evidence = Evidence::default();
    let why = match error {
        ShellError::RoundTooSmall { region } => {
            in_place::draw_region(&mut evidence, solid, &place.topology, region);
            message::ShellRefusal::RoundTooSmall
        }
        ShellError::TooThick => message::ShellRefusal::TooThick,
        ShellError::Corner { vertex } => {
            evidence.add_points(solid.mesh().verts().get(vertex as usize).copied());
            message::ShellRefusal::Corner
        }
        ShellError::Failed(failure) => {
            let words = message::shelling(place.name, failure.error);
            let failure = KernelFailure::new(failure, tolerance);
            return Failed::kernel(words, &failure, [&[place.body], &[]]);
        }
    };
    Failed {
        message: message::shell_refused(why, place.name),
        geometry: ErrorGeometry::of_evidence(&evidence, tolerance),
    }
}

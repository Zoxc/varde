//! Evaluating a shell: the body's solid, as the features before it
//! leave it, hollowed by the kernel's [`varde_kernel::shell()`] and opened
//! through the faces it names.
//!
//! The open faces are found on the body's topology (the one drawing it
//! keeps, [`inspect::topology`]) by their keys and points, as a split's
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

use std::sync::Arc;

use varde_document::{BodyId, Document, FeatureId, Shell};
use varde_kernel::{Budget, Evidence, ShellError, Solid, Tolerance, Topology};

use super::{Evaluation, Failed, own_solids};
use crate::cache::{Cache, Keyer};
use crate::error_geometry::KernelFailure;
use crate::message;
use crate::{ErrorGeometry, inspect};

/// The kernel's shell, which tests may replace with a stand-in to check
/// what regeneration does with the result before the kernel's is built.
type Sheller =
    fn(&Solid, &Topology, &[u32], f64, bool, u64, &Tolerance, &Budget) -> Result<Solid, ShellError>;

#[cfg(any(test, feature = "testing"))]
thread_local! {
    /// The shell a test asks for in place of the kernel's, on its own
    /// thread.
    pub(crate) static SHELLER: std::cell::Cell<Option<Sheller>> =
        const { std::cell::Cell::new(None) };
}

/// The shell to run: the kernel's, or the one a test set.
fn sheller() -> Sheller {
    #[cfg(any(test, feature = "testing"))]
    if let Some(sheller) = SHELLER.get() {
        return sheller;
    }
    varde_kernel::shell
}

/// Shells on this thread by [`by_boxes`] from now on.
#[cfg(any(test, feature = "testing"))]
pub(crate) fn shell_by_boxes() {
    SHELLER.set(Some(by_boxes));
}

/// A stand-in for the kernel's shell, for tests: the solid must be a
/// box along the world's axes (six planar faces square to them, its
/// volume its box's), and is hollowed by one boolean with another box:
/// inward, the body less its box shrunk by the thickness (pushed out
/// past the open faces); outward, its box grown by the thickness (but
/// at the open faces) less the body's box pushed out past the open
/// faces. Anything else is [`KernelError::TooComplex`]; walls meeting
/// inside is [`ShellError::TooThick`].
///
/// [`KernelError::TooComplex`]: varde_kernel::KernelError::TooComplex
#[cfg(any(test, feature = "testing"))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn by_boxes(
    solid: &Solid,
    topology: &Topology,
    open: &[u32],
    thickness: f64,
    outward: bool,
    feature: u64,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<Solid, ShellError> {
    use glam::{DVec2, DVec3};
    use varde_kernel::mesh::Form;
    use varde_kernel::{Frame, KernelError, Loop, Op, Profile, Segment};
    let too_complex = || ShellError::Failed(KernelError::TooComplex.into());
    let bounds = solid.bounds3().ok_or_else(too_complex)?;
    let (min, max) = (bounds.min, bounds.max);
    let size = max - min;
    if topology.regions().len() != 6
        || (solid.volume() - size.x * size.y * size.z).abs() > 1e-9 * size.x * size.y * size.z
    {
        return Err(too_complex());
    }
    // Each region's side of the box: the axis, and 0 at the low end, 1
    // at the high.
    let mut sides = Vec::with_capacity(6);
    for region in topology.regions() {
        let Form::Plane { n, .. } = *crate::picking::region_form(solid, region) else {
            return Err(too_complex());
        };
        let n = n.normalize();
        let axis = n.abs().max_position();
        if (n.abs()[axis] - 1.0).abs() > 1e-12 {
            return Err(too_complex());
        }
        sides.push((axis, usize::from(n[axis] > 0.0)));
    }
    let mut opened = [[false; 2]; 3];
    for &region in open {
        let (axis, end) = *sides.get(region as usize).ok_or_else(too_complex)?;
        opened[axis][end] = true;
    }
    let margin = thickness + 1.0;
    // The box from `lo` to `hi`, an extrude along z.
    let block = |lo: DVec3, hi: DVec3| -> Result<Solid, ShellError> {
        let corners = [
            DVec2::new(lo.x, lo.y),
            DVec2::new(hi.x, lo.y),
            DVec2::new(hi.x, hi.y),
            DVec2::new(lo.x, hi.y),
        ];
        let segments = (0..4)
            .map(|i| Segment::line(corners[i], corners[(i + 1) % 4], i as u64))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| too_complex())?;
        let profile = Profile {
            loops: vec![Loop { segments }],
        };
        Ok(varde_kernel::extrude(
            &profile,
            &Frame::XY,
            lo.z,
            hi.z,
            feature,
            tol,
            budget,
        )?)
    };
    // The body's box moved by `by` at each kept side and pushed out past
    // each open one.
    let moved = |by: f64| {
        let (mut lo, mut hi) = (min, max);
        for axis in 0..3 {
            lo[axis] = if opened[axis][0] {
                min[axis] - margin
            } else {
                min[axis] - by
            };
            hi[axis] = if opened[axis][1] {
                max[axis] + margin
            } else {
                max[axis] + by
            };
        }
        (lo, hi)
    };
    let result = if outward {
        let (mut lo, mut hi) = moved(thickness);
        // The wall stops at the open faces.
        for axis in 0..3 {
            if opened[axis][0] {
                lo[axis] = min[axis];
            }
            if opened[axis][1] {
                hi[axis] = max[axis];
            }
        }
        let (in_lo, in_hi) = moved(0.0);
        let outer = block(lo, hi)?;
        varde_kernel::boolean(&outer, &block(in_lo, in_hi)?, Op::Difference, tol, budget)?
    } else {
        let (lo, hi) = moved(-thickness);
        if (0..3).any(|axis| lo[axis] >= hi[axis]) {
            return Err(ShellError::TooThick);
        }
        varde_kernel::boolean(solid, &block(lo, hi)?, Op::Difference, tol, budget)?
    };
    Ok(result)
}

/// Changes the body of `evaluation` as the shell `shell`, the feature
/// `feature`, says, or says why it fails, changing nothing.
///
/// Its body must have a solid of its own ([`own_solids`]: one a join or
/// a combine consumed fails it, naming the body holding it). Then the
/// faces and the kernel's shell, as the module's docs say.
pub(super) fn evaluate_shell(
    document: &Document,
    feature: FeatureId,
    shell: &Shell,
    tolerance: &Tolerance,
    evaluation: &mut Evaluation,
    cache: &mut Cache,
) -> Result<(), Failed> {
    let body = shell.body;
    own_solids(document, std::iter::once(body), evaluation)?;
    let body_name = document
        .body(body)
        .map_or("a body", |body| body.name.as_str());
    let made = (evaluation.bodies.iter())
        .find(|made| made.body == body)
        .expect("the body has a solid of its own");
    let solid = Arc::clone(&made.solid);
    let topology = inspect::topology(made, cache);
    let count = shell.open.len();
    let mut open = Vec::with_capacity(count);
    for (i, face) in shell.open.iter().enumerate() {
        let region = (topology.face(&solid, &face.key, face.near))
            .map_err(|_| message::shell_face_not_found(i, count))?;
        open.push(region);
    }
    open.sort_unstable();
    open.dedup();
    let thickness = shell.thickness.value;
    let mut keyer = Keyer::new("shell");
    keyer
        .key(made.key)
        .number(feature.get())
        .number(tolerance.fit().to_bits())
        .number(thickness.to_bits())
        .number(u64::from(shell.outward))
        .number(open.len() as u64);
    for &region in &open {
        keyer.number(u64::from(region));
    }
    let key = keyer.finish();
    let shell_by = sheller();
    let result = cache.solid(key, || {
        let budget = &Budget::DEFAULT;
        let shelled = shell_by(
            &solid,
            &topology,
            &open,
            thickness,
            shell.outward,
            feature.get(),
            tolerance,
            budget,
        )
        .map_err(|error| refused(error, body, body_name, &solid, &topology, tolerance))?;
        if shelled.is_empty() {
            return Err(message::shell_leaves_nothing(body_name).into());
        }
        Ok(shelled)
    })?;
    if let Some(made) = evaluation.bodies.iter_mut().find(|made| made.body == body) {
        made.solid = result;
        made.key = key;
    }
    Ok(())
}

/// Why the kernel's shell of the body `body`, named `body_name`, gave no
/// solid, in words, with what to draw: the face or corner a refusal is
/// about, or the failure's evidence.
fn refused(
    error: ShellError,
    body: BodyId,
    body_name: &str,
    solid: &Solid,
    topology: &Topology,
    tolerance: &Tolerance,
) -> Failed {
    let mesh = solid.mesh();
    let mut evidence = Evidence::default();
    let why = match error {
        ShellError::RoundTooSmall { region } => {
            if let Some(region) = topology.regions().get(region as usize) {
                evidence.add_patches(
                    (region.tris.iter())
                        .filter(|&&t| (t as usize) < mesh.tris().len())
                        .map(|&t| mesh.patch(t as usize)),
                );
            }
            message::ShellRefusal::RoundTooSmall
        }
        ShellError::TooThick => message::ShellRefusal::TooThick,
        ShellError::Corner { vertex } => {
            evidence.add_points(mesh.verts().get(vertex as usize).copied());
            message::ShellRefusal::Corner
        }
        ShellError::Failed(failure) => {
            let words = message::shelling(body_name, failure.error);
            let failure = KernelFailure::new(failure, tolerance);
            return Failed::kernel(words, &failure, [&[body], &[]]);
        }
    };
    Failed {
        message: message::shell_refused(why, body_name),
        geometry: ErrorGeometry::of_evidence(&evidence, tolerance),
    }
}

//! Evaluating an offset face: the body's solid, as the features before
//! it leave it, with the faces it names moved along their normals by
//! the kernel's [`varde_kernel::offset_faces()`].
//!
//! The faces are found on the body's topology (the one drawing it
//! keeps, [`inspect::topology`]) by their keys and points, as a shell's
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

use std::sync::Arc;

use varde_document::{BodyId, Document, FeatureId, OffsetFace};
use varde_kernel::{Budget, Evidence, OffsetError, Solid, Tolerance, Topology};

use super::{Evaluation, Failed, own_solids};
use crate::cache::{Cache, Keyer};
use crate::error_geometry::KernelFailure;
use crate::message;
use crate::{ErrorGeometry, inspect};

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
/// a box along the world's axes (six planar faces square to them, its
/// volume its box's), and each face picked moves along its normal by
/// `distance`, the four around it stretched to meet it: one scale along
/// each axis and a move, so every face keeps its name (as the kernel's
/// keeps them). A face moved onto or past the face opposite it (the
/// walls between shrinking to nothing) is
/// [`OffsetError::PastNeighbour`], a box past
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
    use varde_kernel::mesh::Form;
    let too_complex = || OffsetError::Failed(KernelError::TooComplex.into());
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
/// Its body must have a solid of its own ([`own_solids`]: one a join or
/// a combine consumed fails it, naming the body holding it). Then the
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
    own_solids(document, std::iter::once(body), evaluation)?;
    let body_name = document
        .body(body)
        .map_or("a body", |body| body.name.as_str());
    let made = (evaluation.bodies.iter())
        .find(|made| made.body == body)
        .expect("the body has a solid of its own");
    let solid = Arc::clone(&made.solid);
    let topology = inspect::topology(made, cache);
    let count = offset.faces.len();
    let mut faces = Vec::with_capacity(count);
    for (i, face) in offset.faces.iter().enumerate() {
        let region = (topology.face(&solid, &face.key, face.near))
            .map_err(|_| message::offset_face_not_found(i, count))?;
        faces.push(region);
    }
    faces.sort_unstable();
    faces.dedup();
    let distance = offset.signed_distance();
    let mut keyer = Keyer::new("offset face");
    keyer
        .key(made.key)
        .number(feature.get())
        .number(tolerance.fit().to_bits())
        .number(distance.to_bits())
        .number(u64::from(offset.tangent))
        .number(faces.len() as u64);
    for &region in &faces {
        keyer.number(u64::from(region));
    }
    let key = keyer.finish();
    let offset_by = offsetter();
    let result = cache.solid(key, || {
        let budget = &Budget::DEFAULT;
        let moved = offset_by(
            &solid,
            &topology,
            &faces,
            distance,
            offset.tangent,
            feature.get(),
            tolerance,
            budget,
        )
        .map_err(|error| refused(error, body, body_name, &solid, &topology, tolerance))?;
        if moved.is_empty() {
            return Err(message::offset_leaves_nothing(body_name).into());
        }
        Ok(moved)
    })?;
    if let Some(made) = evaluation.bodies.iter_mut().find(|made| made.body == body) {
        made.solid = result;
        made.key = key;
    }
    Ok(())
}

/// Why the kernel's offset of faces of the body `body`, named
/// `body_name`, gave no solid, in words, with what to draw: the face or
/// corner a refusal is about, or the failure's evidence.
fn refused(
    error: OffsetError,
    body: BodyId,
    body_name: &str,
    solid: &Solid,
    topology: &Topology,
    tolerance: &Tolerance,
) -> Failed {
    let mesh = solid.mesh();
    let mut evidence = Evidence::default();
    let mut region_drawn = |region: u32| {
        if let Some(region) = topology.regions().get(region as usize) {
            evidence.add_patches(
                (region.tris.iter())
                    .filter(|&&t| (t as usize) < mesh.tris().len())
                    .map(|&t| mesh.patch(t as usize)),
            );
        }
    };
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
            evidence.add_points(mesh.verts().get(vertex as usize).copied());
            message::OffsetRefusal::Corner
        }
        OffsetError::OutOfRange => message::OffsetRefusal::OutOfRange,
        OffsetError::Failed(failure) => {
            let words = message::offsetting(body_name, failure.error);
            let failure = KernelFailure::new(failure, tolerance);
            return Failed::kernel(words, &failure, [&[body], &[]]);
        }
    };
    Failed {
        message: message::offset_refused(why, body_name),
        geometry: ErrorGeometry::of_evidence(&evidence, tolerance),
    }
}

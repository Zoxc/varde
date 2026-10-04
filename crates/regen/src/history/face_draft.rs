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

/// The kernel's draft, which tests may replace with a stand-in to check
/// what regeneration does with the result before the kernel's is built.
type Drafter = fn(
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

#[cfg(any(test, feature = "testing"))]
thread_local! {
    /// The draft a test asks for in place of the kernel's, on its own
    /// thread.
    pub(crate) static DRAFTER: std::cell::Cell<Option<Drafter>> =
        const { std::cell::Cell::new(None) };
}

/// The draft to run: the kernel's, or the one a test set.
fn drafter() -> Drafter {
    #[cfg(any(test, feature = "testing"))]
    if let Some(drafter) = DRAFTER.get() {
        return drafter;
    }
    varde_kernel::draft_faces
}

/// Drafts faces on this thread by [`by_boxes`] from now on.
#[cfg(any(test, feature = "testing"))]
pub(crate) fn draft_by_boxes() {
    DRAFTER.set(Some(by_boxes));
}

/// A stand-in for the kernel's draft, for tests: the solid must be a box
/// along the world's axes ([`in_place::box_sides`]) of eight corners,
/// and the pull along a world axis. Each face picked must be one of the
/// four sides along the pull (a top or bottom faces it:
/// [`DraftError::FacingPull`]); it turns about its hinge line on the
/// neutral plane, its corners moved across the pull by `tan α` times
/// their height above the plane (inward above it, outward below), so it
/// stays flat on the turned plane and the faces around it stay on
/// theirs, every face keeping its name. Sides drafted until they meet
/// the one opposite at the box's top or bottom are
/// [`DraftError::PastNeighbour`], a box past
/// [`MAX_COORD`](varde_kernel::MAX_COORD) [`DraftError::OutOfRange`];
/// anything else is [`KernelError::TooComplex`]. The result passes the
/// kernel's check as every solid does.
///
/// [`KernelError::TooComplex`]: varde_kernel::KernelError::TooComplex
#[cfg(any(test, feature = "testing"))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn by_boxes(
    solid: &Solid,
    topology: &Topology,
    faces: &[u32],
    neutral: DVec3,
    pull: DVec3,
    angle: f64,
    _tangent: bool,
    _feature: u64,
    tol: &Tolerance,
    _budget: &Budget,
) -> Result<Solid, DraftError> {
    use varde_kernel::KernelError;
    use varde_kernel::mesh::{Edge, Form, Mesh, Surface};
    let too_complex = || DraftError::Failed(KernelError::TooComplex.into());
    let (min, max, sides) = in_place::box_sides(solid, topology).ok_or_else(too_complex)?;
    let mesh = solid.mesh();
    let up = pull.abs().max_position();
    if (pull.abs()[up] - 1.0).abs() > 1e-12 || mesh.verts().len() != 8 {
        return Err(too_complex());
    }
    let sign = pull[up].signum();
    let (sin, cos) = varde_kernel::trig::sin_cos(angle);
    let tan = sin / cos;
    // Each axis's low and high side: the region drafted there, if any.
    let mut drafted = [[None; 2]; 3];
    for &region in faces {
        let (axis, end) = *sides.get(region as usize).ok_or_else(too_complex)?;
        if axis == up {
            return Err(DraftError::FacingPull { region });
        }
        drafted[axis][end] = Some(region);
    }
    // A point's height above the neutral plane, along the pull: a side
    // drafted moves inward by `tan α` times it there.
    let height = |x: DVec3| sign * (x[up] - neutral[up]);
    // The sides meeting the ones opposite them, at either end of the box
    // along the pull (the width is linear in the height between).
    let least = tol.resolution();
    for t in [height(min), height(max)] {
        for (axis, ends) in drafted.iter().enumerate() {
            let shrink = ends.iter().flatten().count() as f64 * tan * t;
            if max[axis] - min[axis] - shrink <= least {
                let region = ends.iter().flatten().next().copied();
                return Err(DraftError::PastNeighbour {
                    region: region.expect("only a drafted side shrinks an axis"),
                });
            }
        }
    }
    let size = (max - min).length();
    let on = |a: f64, b: f64| (a - b).abs() <= 1e-9 * (1.0 + size);
    let verts: Vec<DVec3> = (mesh.verts().iter())
        .map(|&v| {
            let mut moved = v;
            let t = height(v);
            for (axis, ends) in drafted.iter().enumerate() {
                if ends[0].is_some() && on(v[axis], min[axis]) {
                    moved[axis] = min[axis] + tan * t;
                }
                if ends[1].is_some() && on(v[axis], max[axis]) {
                    moved[axis] = max[axis] - tan * t;
                }
            }
            moved
        })
        .collect();
    let reach = f64::from(varde_kernel::MAX_COORD);
    if verts.iter().any(|v| v.abs().max_element() > reach) {
        return Err(DraftError::OutOfRange);
    }
    let mut edges = mesh.edges().to_vec();
    for tri in mesh.tris() {
        for (i, half) in tri.halfedges.iter().enumerate() {
            let end = tri.halfedges[(i + 1) % 3].start;
            edges[half.edge as usize] =
                Edge::straight(verts[half.start as usize], verts[end as usize]);
        }
    }
    let mut new_faces = mesh.faces().to_vec();
    for (region, &(axis, end)) in sides.iter().enumerate() {
        if drafted[axis][end] != Some(region as u32) {
            continue;
        }
        // The hinge: on the side's plane and the neutral plane.
        let s = if end == 1 { 1.0 } else { -1.0 };
        let c = if end == 1 { max[axis] } else { min[axis] };
        let mut n = pull * sin;
        n[axis] = s * cos;
        let d = s * cos * c + sin * sign * neutral[up];
        let key = topology.regions()[region].key;
        for face in new_faces.iter_mut().filter(|face| face.name.key() == key) {
            face.form = Form::Plane { n, d };
            face.surface = Surface::Plane { n, d };
        }
    }
    let drafted_mesh = Mesh::from_parts(verts, edges, mesh.tris().to_vec(), new_faces)
        .with_aliases(mesh.aliases().to_vec());
    Solid::new(drafted_mesh, tol).map_err(|error| DraftError::Failed(error.into()))
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

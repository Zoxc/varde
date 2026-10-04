//! Evaluating a fillet: the body's solid, as the features before it
//! leave it, with the fillet's edges rounded off by the kernel's
//! [`varde_kernel::fillet()`].
//!
//! The edges are found, grown along tangent chains and named as a
//! chamfer's are (`super::blend`; a fillet, of one radius, has no use
//! for the chains' first faces).
//!
//! The result is cached by the body's key, the chains, the radius's
//! bits, the feature and the fit tolerance, and replaces the body's
//! solid (the body keeps its id). The kernel's refusals are worded for
//! the Timeline (`message::blend_refused`), with the edge they're about
//! drawn; its other failures as a boolean's ("filleting Body 1 ...").
//!
//! The kernel's fillet isn't built yet: it fails as too complex, which
//! reaches the user as "filleting Body 1 is too complex to work out",
//! and the rest of the history goes on.

use varde_document::{Document, FeatureId, Fillet};
use varde_kernel::{BlendError, Budget, FilletChain, Solid, Tolerance, Topology};

use super::blend::{find_edges, plan, refused};
use super::own_body::OwnBody;
use super::{Evaluation, Failed};
use crate::cache::{Cache, Keyer};
use crate::message::{self, Blend};

/// The kernel's fillet, which tests may replace with a stand-in to check
/// what regeneration does with the result before the kernel's is built.
type Filleter = fn(
    &Solid,
    &Topology,
    &[FilletChain],
    f64,
    u64,
    &Tolerance,
    &Budget,
) -> Result<Solid, BlendError>;

#[cfg(any(test, feature = "testing"))]
thread_local! {
    /// The fillet a test asks for in place of the kernel's, on its own
    /// thread.
    pub(crate) static FILLETER: std::cell::Cell<Option<Filleter>> =
        const { std::cell::Cell::new(None) };
}

/// The fillet to run: the kernel's, or the one a test set.
fn filleter() -> Filleter {
    #[cfg(any(test, feature = "testing"))]
    if let Some(filleter) = FILLETER.get() {
        return filleter;
    }
    varde_kernel::fillet
}

/// Fillets on this thread by [`by_arcs`] from now on.
#[cfg(any(test, feature = "testing"))]
pub(crate) fn fillet_by_arcs() {
    FILLETER.set(Some(by_arcs));
}

/// A stand-in for the kernel's fillet, for tests: each chain must be a
/// straight, open, convex edge between two planar faces, and is rounded
/// off by cutting away a prism running past its ends whose section is
/// the corner between the edge and the two rails (where the round
/// touches each face) less the round's circle, widened outwards along
/// each face's normal: one boolean each. Right where the faces at the
/// chain's ends are square to it (a block's edges); where two rounds
/// meet at a corner the booleans find them tangent there and fail (the
/// kernel's mitres them). Anything else is [`KernelError::TooComplex`],
/// and a rail past the face it's on is [`BlendError::TooBig`] as far as
/// the stand-in can tell (past the edge's own faces' extent along the
/// section).
///
/// [`KernelError::TooComplex`]: varde_kernel::KernelError::TooComplex
#[cfg(any(test, feature = "testing"))]
pub(crate) fn by_arcs(
    solid: &Solid,
    topology: &Topology,
    chains: &[FilletChain],
    radius: f64,
    feature: u64,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<Solid, BlendError> {
    use glam::{DVec2, DVec3};
    use varde_kernel::mesh::Form;
    use varde_kernel::patch::Conic2;
    use varde_kernel::{Frame, KernelError, Loop, Op, Profile, Segment};
    let too_complex = || BlendError::Failed(KernelError::TooComplex.into());
    let mesh = solid.mesh();
    let mut result = solid.clone();
    for filleted in chains {
        let chain = (topology.chains())
            .get(filleted.chain as usize)
            .ok_or_else(too_complex)?;
        let (Some(&first), Some(&last)) = (chain.halfedges.first(), chain.halfedges.last()) else {
            return Err(too_complex());
        };
        if chain.closed {
            return Err(too_complex());
        }
        let a = mesh.curve(first).p0;
        let b = mesh.curve(last).p1;
        let length = (b - a).length();
        let t = (b - a) / length;
        // Into each face from the edge, square to it, each face's
        // outward normal, and how far the face reaches across.
        let mut into = [DVec3::ZERO; 2];
        let mut normal = [DVec3::ZERO; 2];
        let mut reach = [0.0f64; 2];
        for (side, &region) in chain.regions.iter().enumerate() {
            let region = &topology.regions()[region as usize];
            let Form::Plane { n, .. } = *crate::picking::region_form(solid, region) else {
                return Err(too_complex());
            };
            let inside = mesh
                .patch(region.tris[0] as usize)
                .eval(DVec3::splat(1.0 / 3.0));
            let across = (inside - a) - t * (inside - a).dot(t);
            into[side] = across.normalize();
            normal[side] = n.normalize();
            reach[side] = (region.tris.iter())
                .flat_map(|&tri| {
                    let patch = mesh.patch(tri as usize);
                    [DVec3::X, DVec3::Y, DVec3::Z]
                        .map(|corner| (patch.eval(corner) - a).dot(into[side]))
                })
                .fold(0.0, f64::max);
        }
        // Convex: each face lies behind the other's plane.
        if into[0].dot(normal[1]) >= 0.0 || into[1].dot(normal[0]) >= 0.0 {
            return Err(too_complex());
        }
        // The angle between the faces, inside the material, and the
        // rails' distance from the edge: `r / tan(θ/2)`.
        let cos = into[0].dot(into[1]).clamp(-1.0, 1.0);
        let (sin_half, cos_half) = (((1.0 - cos) / 2.0).sqrt(), ((1.0 + cos) / 2.0).sqrt());
        let back = radius * cos_half / sin_half;
        if !(back.is_finite() && back > 0.0) {
            return Err(too_complex());
        }
        if reach.iter().any(|&reach| back >= reach) {
            return Err(BlendError::TooBig {
                chain: filleted.chain,
            });
        }
        let x = into[0];
        let y = t.cross(x);
        let frame = Frame { origin: a, x, y };
        let flat = |p: DVec3| DVec2::new((p - a).dot(x), (p - a).dot(y));
        let rails = [flat(a + into[0] * back), flat(a + into[1] * back)];
        let centre = flat(a + (into[0] + into[1]).normalize() * (radius / sin_half));
        let margin = back + radius + 1.0;
        let out = [
            rails[0] + flat(a + normal[0] * margin),
            rails[1] + flat(a + normal[1] * margin),
        ];
        // The loop rail 0, out along face 0's normal, across, back in to
        // rail 1, and the arc back to rail 0, counter-clockwise.
        let line =
            |p: DVec2, q: DVec2, curve| Segment::line(p, q, curve).map_err(|_| too_complex());
        let arc = |p: DVec2, q: DVec2| {
            Conic2::arc_between(centre, radius, p, q)
                .map(|conic| Segment { conic, curve: 3 })
                .map_err(|_| too_complex())
        };
        let corners = [rails[0], out[0], out[1], rails[1]];
        let area: f64 = (0..4)
            .map(|i| corners[i].perp_dot(corners[(i + 1) % 4]))
            .sum();
        let segments = if area > 0.0 {
            vec![
                line(rails[0], out[0], 0)?,
                line(out[0], out[1], 1)?,
                line(out[1], rails[1], 2)?,
                arc(rails[1], rails[0])?,
            ]
        } else {
            vec![
                line(rails[1], out[1], 2)?,
                line(out[1], out[0], 1)?,
                line(out[0], rails[0], 0)?,
                arc(rails[0], rails[1])?,
            ]
        };
        let profile = Profile {
            loops: vec![Loop { segments }],
        };
        let notch = varde_kernel::extrude(
            &profile,
            &frame,
            -margin,
            length + margin,
            feature,
            tol,
            budget,
        )?;
        result = varde_kernel::boolean(&result, &notch, Op::Difference, tol, budget)?;
    }
    Ok(result)
}

/// Changes the body of `evaluation` as the fillet `fillet`, the feature
/// `feature`, says, or says why it fails, changing nothing.
///
/// Its body must have a solid of its own (a join or a combine consumed
/// fails it, naming the body holding it). Then the edges, the chains and
/// the kernel's fillet, as the module's docs say.
pub(super) fn evaluate_fillet(
    document: &Document,
    feature: FeatureId,
    fillet: &Fillet,
    tolerance: &Tolerance,
    evaluation: &mut Evaluation,
    cache: &mut Cache,
) -> Result<(), Failed> {
    // A checked document's fillet has edges, all on one body.
    let Some(body) = fillet.body() else {
        return Ok(());
    };
    let own = OwnBody::take(document, body, evaluation, cache)?;
    let (solid, topology) = (&own.solid, &own.topology);
    let found = find_edges(&own, &fillet.edges)?;
    let planned = plan(solid, topology, &fillet.edges, &found, fillet.chains, false);
    let chains: Vec<FilletChain> = (planned.iter())
        .map(|planned| FilletChain {
            chain: planned.chain,
            name: planned.name,
        })
        .collect();
    let radius = fillet.radius.value;
    let mut keyer = Keyer::new("fillet");
    keyer
        .key(own.key)
        .number(feature.get())
        .number(tolerance.fit().to_bits())
        .number(radius.to_bits())
        .number(chains.len() as u64);
    for chain in &chains {
        keyer.number(u64::from(chain.chain)).number(chain.name);
    }
    let key = keyer.finish();
    let fillet_by = filleter();
    let count = fillet.edges.len();
    let result = cache.solid(key, || {
        let budget = &Budget::DEFAULT;
        let filleted = fillet_by(
            solid,
            topology,
            &chains,
            radius,
            feature.get(),
            tolerance,
            budget,
        )
        .map_err(|error| refused(Blend::Fillet, error, &own, &planned, tolerance, count))?;
        if filleted.is_empty() {
            return Err(message::blend_leaves_nothing(Blend::Fillet, own.name).into());
        }
        Ok(filleted)
    })?;
    own.replace(evaluation, result, key);
    Ok(())
}

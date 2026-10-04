//! Evaluating a chamfer: the body's solid, as the features before it
//! leave it, with the chamfer's edges cut off by the kernel's
//! [`varde_kernel::chamfer`].
//!
//! The edges are found, grown along tangent chains, given their first
//! faces and named as a fillet's are (`super::blend`).
//!
//! The result is cached by the body's key, the chains and how each is
//! cut, the feature and the fit tolerance, and replaces the body's solid
//! (the body keeps its id). The kernel's refusals are worded for the
//! Timeline (`message::blend_refused`), with the edge they're about
//! drawn; its other failures as a boolean's ("chamfering Body 1 ...").
//!
//! The kernel's chamfer isn't built yet: it fails as too complex, which
//! reaches the user as "chamfering Body 1 is too complex to work out",
//! and the rest of the history goes on.

use varde_document::{Chamfer, ChamferSize, Document, FeatureId};
use varde_kernel::{BlendError, Budget, ChamferChain, ChamferCut, Solid, Tolerance, Topology};

use super::blend::{OwnBody, plan, refused};
use super::{Evaluation, Failed};
use crate::cache::{Cache, Keyer};
use crate::message::{self, Blend};

/// The kernel's chamfer, which tests may replace with a stand-in to
/// check what regeneration does with the result before the kernel's is
/// built.
type Chamferer =
    fn(&Solid, &Topology, &[ChamferChain], u64, &Tolerance, &Budget) -> Result<Solid, BlendError>;

#[cfg(any(test, feature = "testing"))]
thread_local! {
    /// The chamfer a test asks for in place of the kernel's, on its own
    /// thread.
    pub(crate) static CHAMFERER: std::cell::Cell<Option<Chamferer>> =
        const { std::cell::Cell::new(None) };
}

/// The chamfer to run: the kernel's, or the one a test set.
fn chamferer() -> Chamferer {
    #[cfg(any(test, feature = "testing"))]
    if let Some(chamferer) = CHAMFERER.get() {
        return chamferer;
    }
    varde_kernel::chamfer
}

/// Chamfers on this thread by [`by_wedges`] from now on.
#[cfg(any(test, feature = "testing"))]
pub(crate) fn chamfer_by_wedges() {
    CHAMFERER.set(Some(by_wedges));
}

/// A stand-in for the kernel's chamfer, for tests: each chain must be a
/// straight, open, convex edge between two planar faces, and is cut off
/// by a triangular prism (the corner between the edge and the two rails,
/// widened outwards) running past its ends, one boolean each. Right
/// where the faces at the chain's ends are square to it (a block's
/// edges); anything else is [`KernelError::TooComplex`], a rail past
/// its face is [`BlendError::TooBig`] only as far as the stand-in can
/// tell (an angled cut that never meets the other face).
///
/// [`KernelError::TooComplex`]: varde_kernel::KernelError::TooComplex
#[cfg(any(test, feature = "testing"))]
pub(crate) fn by_wedges(
    solid: &Solid,
    topology: &Topology,
    chains: &[ChamferChain],
    feature: u64,
    tol: &Tolerance,
    budget: &Budget,
) -> Result<Solid, BlendError> {
    use glam::DVec3;
    use varde_kernel::mesh::Form;
    use varde_kernel::{Frame, KernelError, Loop, Op, Profile, Segment};
    let too_complex = || BlendError::Failed(KernelError::TooComplex.into());
    let mesh = solid.mesh();
    let mut result = solid.clone();
    for chamfered in chains {
        let chain = (topology.chains())
            .get(chamfered.chain as usize)
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
        // Into each face from the edge, square to it, and each face's
        // normal.
        let mut into = [DVec3::ZERO; 2];
        let mut normal = [DVec3::ZERO; 2];
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
        }
        // Convex: each face lies behind the other's plane.
        if into[0].dot(normal[1]) >= 0.0 || into[1].dot(normal[0]) >= 0.0 {
            return Err(too_complex());
        }
        let [d0, d1] = match chamfered.cut {
            ChamferCut::Distances(distances) => distances,
            ChamferCut::Angle {
                on,
                distance,
                angle,
            } => {
                // The cut from `distance` along face `on`, at `angle` to it
                // inside the material, meets the other face this far
                // along: in the section's frame of `into[on]` and the
                // unit `m` square to it towards the other face.
                let other = into[1 - on];
                let c1 = other.dot(into[on]);
                let m = (other - into[on] * c1).normalize();
                let c2 = other.dot(m);
                let (sin, cos) = varde_kernel::trig::sin_cos(angle);
                let k = distance * sin / (c1 * sin + c2 * cos);
                if !(k.is_finite() && k > 0.0) {
                    return Err(BlendError::TooBig {
                        chain: chamfered.chain,
                    });
                }
                if on == 0 {
                    [distance, k]
                } else {
                    [k, distance]
                }
            }
        };
        let rails = [a + into[0] * d0, a + into[1] * d1];
        let apex = a - (into[0] + into[1]) * (d0 + d1);
        let x = into[0];
        let y = t.cross(x);
        let frame = Frame { origin: a, x, y };
        let flat = |p: DVec3| glam::DVec2::new((p - a).dot(x), (p - a).dot(y));
        let mut corners = [flat(rails[0]), flat(rails[1]), flat(apex)];
        let [p, q, r] = corners;
        if (q - p).perp_dot(r - p) < 0.0 {
            corners.swap(0, 1);
        }
        let segments = (0..3)
            .map(|i| Segment::line(corners[i], corners[(i + 1) % 3], i as u64))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| too_complex())?;
        let profile = Profile {
            loops: vec![Loop { segments }],
        };
        let margin = d0 + d1 + 1.0;
        let prism = varde_kernel::extrude(
            &profile,
            &frame,
            -margin,
            length + margin,
            feature,
            tol,
            budget,
        )?;
        result = varde_kernel::boolean(&result, &prism, Op::Difference, tol, budget)?;
    }
    Ok(result)
}

/// Changes the body of `evaluation` as the chamfer `chamfer`, the
/// feature `feature`, says, or says why it fails, changing nothing.
///
/// Its body must have a solid of its own (`own_solids`: one a join or
/// a combine consumed fails it, naming the body holding it). Then the
/// edges, the chains and the kernel's chamfer, as the module's docs say.
pub(super) fn evaluate_chamfer(
    document: &Document,
    feature: FeatureId,
    chamfer: &Chamfer,
    tolerance: &Tolerance,
    evaluation: &mut Evaluation,
    cache: &mut Cache,
) -> Result<(), Failed> {
    // A checked document's chamfer has edges, all on one body.
    let Some(body) = chamfer.body() else {
        return Ok(());
    };
    let own = OwnBody::take(document, body, evaluation, cache)?;
    let (solid, topology) = (&own.solid, &own.topology);
    let found = own.edges(&chamfer.edges)?;
    let planned = plan(
        solid,
        topology,
        &chamfer.edges,
        &found,
        chamfer.chains,
        chamfer.flip,
    );
    let chains: Vec<ChamferChain> = (planned.iter())
        .map(|planned| ChamferChain {
            chain: planned.chain,
            name: planned.name,
            cut: cut(
                &chamfer.distances,
                topology.chains()[planned.chain as usize].regions,
                planned.first,
            ),
        })
        .collect();
    let mut keyer = Keyer::new("chamfer");
    keyer
        .key(own.key)
        .number(feature.get())
        .number(tolerance.fit().to_bits());
    for chain in &chains {
        keyer.number(u64::from(chain.chain)).number(chain.name);
        match chain.cut {
            ChamferCut::Distances([a, b]) => {
                keyer.number(0).number(a.to_bits()).number(b.to_bits())
            }
            ChamferCut::Angle {
                on,
                distance,
                angle,
            } => keyer
                .number(1)
                .number(on as u64)
                .number(distance.to_bits())
                .number(angle.to_bits()),
        };
    }
    let key = keyer.finish();
    let chamfer_by = chamferer();
    let count = chamfer.edges.len();
    let result = cache.solid(key, || {
        let budget = &Budget::DEFAULT;
        let chamfered = chamfer_by(solid, topology, &chains, feature.get(), tolerance, budget)
            .map_err(|error| refused(Blend::Chamfer, error, &own, &planned, tolerance, count))?;
        if chamfered.is_empty() {
            return Err(message::blend_leaves_nothing(Blend::Chamfer, own.name).into());
        }
        Ok(chamfered)
    })?;
    own.replace(evaluation, result, key);
    Ok(())
}

/// How a chain with regions `sides` and first face `first` is cut by
/// `size`, in the kernel's terms (its distances in the regions' order).
fn cut(size: &ChamferSize, sides: [u32; 2], first: u32) -> ChamferCut {
    let on = usize::from(sides[0] != first);
    match size {
        ChamferSize::Equal(d) => ChamferCut::Distances([d.value; 2]),
        ChamferSize::Two(a, b) => {
            let mut distances = [a.value, b.value];
            if on == 1 {
                distances.reverse();
            }
            ChamferCut::Distances(distances)
        }
        ChamferSize::Angle(d, a) => ChamferCut::Angle {
            on,
            distance: d.value,
            angle: a.value,
        },
    }
}

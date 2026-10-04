//! Evaluating a chamfer: the body's solid, as the features before it
//! leave it, with the chamfer's edges cut off by the kernel's
//! [`varde_kernel::chamfer`].
//!
//! The edges are found on the body's topology (the one drawing it keeps,
//! [`inspect::topology`]) by their faces' keys and points, as a scale's
//! edge is; one not found fails the chamfer ("its edge wasn't found",
//! "its edge 2 of 3 wasn't found"). With tangent chains on, each edge
//! takes in the chains running on smoothly from it
//! ([`Topology::tangent_chains`]: purely the topology and the curves'
//! end tangents, so it's done here rather than in the kernel). A chain
//! is chamfered once: the edges picked first, in the chamfer's order,
//! then those grown into, each taken by the first edge reaching it.
//!
//! Each chain's **first face**, which a two-distance cut's first
//! distance and an angled cut's distance and angle are taken along: for
//! a picked edge, the region its reference's first key names (its
//! second with `flip`; by the region's own key where aliases name both
//! by both keys, else the lower region); for one grown into, the region
//! it shares with its picked edge's first face, else the other region
//! beside the one it shares with the picked edge's other face, else its
//! lower-keyed region. Each chain's faces are named
//! [`FacePart::Blend`](varde_kernel::mesh::FacePart::Blend) of
//! [`blend_edge`]: a picked edge's of its reference's keys, a grown
//! one's of its regions' keys, the ordinal counting the chains before it
//! in that order with the same pair.
//!
//! The result is cached by the body's key, the chains and how each is
//! cut, the feature and the fit tolerance, and replaces the body's solid
//! (the body keeps its id). The kernel's refusals are worded for the
//! Timeline (`message::chamfer_refused`), with the edge they're about
//! drawn; its other failures as a boolean's ("chamfering Body 1 ...").
//!
//! The kernel's chamfer isn't built yet: it fails as too complex, which
//! reaches the user as "chamfering Body 1 is too complex to work out",
//! and the rest of the history goes on.

use std::sync::Arc;

use varde_document::{Chamfer, ChamferSize, Document, EdgeRef, FeatureId};
use varde_kernel::mesh::FaceKey;
use varde_kernel::topology::blend_edge;
use varde_kernel::{
    BlendError, Budget, ChamferChain, ChamferCut, Evidence, Solid, Tolerance, Topology,
};

use super::{Evaluation, Failed, own_solids};
use crate::cache::{Cache, Keyer};
use crate::error_geometry::KernelFailure;
use crate::message;
use crate::{ErrorGeometry, inspect};

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

/// One chain to chamfer as it's worked out: what the kernel gets, the
/// pair of keys its name is made of, and which of the chamfer's edges
/// it comes from and whether it was grown into, for the messages.
struct Planned {
    chain: ChamferChain,
    pair: [FaceKey; 2],
    edge: usize,
    grown: bool,
}

/// Changes the body of `evaluation` as the chamfer `chamfer`, the
/// feature `feature`, says, or says why it fails, changing nothing.
///
/// Its body must have a solid of its own ([`own_solids`]: one a join or
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
    own_solids(document, std::iter::once(body), evaluation)?;
    let body_name = document
        .body(body)
        .map_or("a body", |body| body.name.as_str());
    let made = (evaluation.bodies.iter())
        .find(|made| made.body == body)
        .expect("the body has a solid of its own");
    let solid = Arc::clone(&made.solid);
    let topology = inspect::topology(made, cache);
    let count = chamfer.edges.len();
    let mut found = Vec::with_capacity(count);
    for (i, edge) in chamfer.edges.iter().enumerate() {
        let chain = (topology.edge(&solid, edge.faces, edge.near))
            .map_err(|_| message::chamfer_edge_not_found(i, count))?;
        found.push(chain);
    }
    let planned = plan(&solid, &topology, chamfer, &found);
    let chains: Vec<ChamferChain> = planned.iter().map(|planned| planned.chain).collect();
    let mut keyer = Keyer::new("chamfer");
    keyer
        .key(made.key)
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
    let result = cache.solid(key, || {
        let budget = &Budget::DEFAULT;
        let chamfered = chamfer_by(&solid, &topology, &chains, feature.get(), tolerance, budget)
            .map_err(|error| {
                refused(
                    error, body, body_name, &planned, &solid, &topology, tolerance, count,
                )
            })?;
        if chamfered.is_empty() {
            return Err(message::chamfer_leaves_nothing(body_name).into());
        }
        Ok(chamfered)
    })?;
    if let Some(made) = evaluation.bodies.iter_mut().find(|made| made.body == body) {
        made.solid = result;
        made.key = key;
    }
    Ok(())
}

/// The chains `chamfer`'s edges, found as the chains `found` of
/// `topology` (of `solid`), come to, as the module's docs say.
fn plan(solid: &Solid, topology: &Topology, chamfer: &Chamfer, found: &[u32]) -> Vec<Planned> {
    let regions = topology.regions();
    let chains = topology.chains();
    let key_of = |region: u32| regions[region as usize].key;
    let sides_of = |chain: u32| chains[chain as usize].regions;
    // Each picked edge's first region.
    let firsts: Vec<u32> = (chamfer.edges.iter().zip(found))
        .map(|(edge, &chain)| first_region(topology, sides_of(chain), edge, chamfer.flip))
        .collect();
    let mut planned: Vec<Planned> = Vec::new();
    let add = |planned: &mut Vec<Planned>, chain: u32, pair: [FaceKey; 2], first, edge, grown| {
        if planned.iter().any(|p| p.chain.chain == chain) {
            return;
        }
        // Ordinals count the chains before it with the same pair.
        let ordinal = planned.iter().filter(|p| p.pair == pair).count();
        planned.push(Planned {
            chain: ChamferChain {
                chain,
                name: blend_edge(pair, ordinal as u32),
                cut: cut(&chamfer.distances, sides_of(chain), first),
            },
            pair,
            edge,
            grown,
        });
    };
    for (i, (edge, &chain)) in chamfer.edges.iter().zip(found).enumerate() {
        add(&mut planned, chain, edge.faces, firsts[i], i, false);
    }
    if chamfer.chains {
        let roots = topology.tangent_chains(solid);
        for (i, &chain) in found.iter().enumerate() {
            let picked = sides_of(chain);
            let first = firsts[i];
            let second = if picked[0] == first {
                picked[1]
            } else {
                picked[0]
            };
            let root = roots[chain as usize];
            for (other, _) in (roots.iter().enumerate()).filter(|&(_, &r)| r == root) {
                let other = other as u32;
                let sides = sides_of(other);
                let other_first = if sides.contains(&first) {
                    first
                } else if let Some(at) = sides.iter().position(|&r| r == second) {
                    sides[1 - at]
                } else if key_of(sides[0]) <= key_of(sides[1]) {
                    sides[0]
                } else {
                    sides[1]
                };
                let mut pair = sides.map(key_of);
                pair.sort();
                add(&mut planned, other, pair, other_first, i, true);
            }
        }
    }
    planned
}

/// The region of the two `sides` of a picked edge's chain that is its
/// first face: the one its reference's first key names (with `flip`,
/// its second), by its own key where aliases name both by both keys,
/// else the lower region.
fn first_region(topology: &Topology, sides: [u32; 2], edge: &EdgeRef, flip: bool) -> u32 {
    let regions = topology.regions();
    let key = edge.faces[usize::from(flip)];
    let named: Vec<u32> = sides
        .into_iter()
        .filter(|&r| regions[r as usize].named(&key))
        .collect();
    match named.as_slice() {
        [one] => *one,
        _ => (sides.into_iter())
            .find(|&r| regions[r as usize].key == key)
            .unwrap_or(sides[0]),
    }
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

/// Why the kernel's chamfer of the body `body`, named `body_name`, gave
/// no solid, in words, with what to draw: the edge a refusal is about,
/// or the failure's evidence.
#[allow(clippy::too_many_arguments)]
fn refused(
    error: BlendError,
    body: varde_document::BodyId,
    body_name: &str,
    planned: &[Planned],
    solid: &Solid,
    topology: &Topology,
    tolerance: &Tolerance,
    count: usize,
) -> Failed {
    let about = |chain: u32| planned.iter().find(|p| p.chain.chain == chain);
    let edge_failed = |chain: u32, why: message::BlendRefusal| {
        let which = about(chain).map(|p| (p.edge, p.grown));
        Failed {
            message: message::chamfer_refused(why, which, count, body_name),
            geometry: chain_geometry(solid, topology, chain, tolerance),
        }
    };
    match error {
        BlendError::Flat { chain } => edge_failed(chain, message::BlendRefusal::Flat),
        BlendError::Folded { chain } => edge_failed(chain, message::BlendRefusal::Folded),
        BlendError::Mixed { chain } => edge_failed(chain, message::BlendRefusal::Mixed),
        BlendError::TooBig { chain } => edge_failed(chain, message::BlendRefusal::TooBig),
        BlendError::Corner { .. } => {
            message::chamfer_refused(message::BlendRefusal::Corner, None, count, body_name).into()
        }
        BlendError::Failed(failure) => {
            let words = message::chamfering(body_name, failure.error);
            let failure = KernelFailure::new(failure, tolerance);
            Failed::kernel(words, &failure, [&[body], &[]])
        }
    }
}

/// What a refusal about `chain` of `topology` (of `solid`) shows: its
/// curves, drawn at the display of `tolerance`.
fn chain_geometry(
    solid: &Solid,
    topology: &Topology,
    chain: u32,
    tolerance: &Tolerance,
) -> Option<Arc<ErrorGeometry>> {
    let chain = topology.chains().get(chain as usize)?;
    let mesh = solid.mesh();
    let tris = mesh.tris().len();
    let mut evidence = Evidence::default();
    evidence.add_curves(
        (chain.halfedges.iter())
            .filter(|&&h| (h as usize) / 3 < tris)
            .map(|&h| mesh.curve(h)),
    );
    ErrorGeometry::of_evidence(&evidence, tolerance)
}

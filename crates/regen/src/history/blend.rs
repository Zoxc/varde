//! What chamfers and fillets share in the history: their body's solid
//! and topology, the edges found on it, the chains those come to (grown
//! along tangent chains where asked) and how they're named, and the
//! kernel's refusals worded with the edge drawn. The shell takes its
//! body the same way ([`OwnBody`]).
//!
//! The edges are found on the body's topology (the one drawing it keeps,
//! [`inspect::topology`]) by their faces' keys and points, as a scale's
//! edge is; one not found fails the feature ("its edge wasn't found",
//! "its edge 2 of 3 wasn't found"). With tangent chains on, each edge
//! takes in the chains running on smoothly from it
//! ([`Topology::tangent_chains`]: purely the topology and the curves'
//! end tangents, so it's done here rather than in the kernel). A chain
//! is handed over once: the edges picked first, in the feature's order,
//! then those grown into, each taken by the first edge reaching it.
//!
//! Each chain's **first face** (what a chamfer's two-distance cut's first
//! distance and an angled cut's distance and angle are taken along; a
//! fillet has no use for it): for a picked edge, the region its
//! reference's first key names (its second with `flip`; by the region's
//! own key where aliases name both by both keys, else the lower region);
//! for one grown into, the region it shares with its picked edge's first
//! face, else the other region beside the one it shares with the picked
//! edge's other face, else its lower-keyed region. Each chain's faces are
//! named [`FacePart::Blend`](varde_kernel::mesh::FacePart::Blend) of
//! [`blend_edge`]: a picked edge's of its reference's keys, a grown
//! one's of its regions' keys, the ordinal counting the chains before it
//! in that order with the same pair.

use std::collections::BTreeMap;
use std::sync::Arc;

use varde_document::{BodyId, Document, EdgeRef};
use varde_kernel::mesh::FaceKey;
use varde_kernel::topology::blend_edge;
use varde_kernel::{BlendError, Evidence, Solid, Tolerance, Topology};

use super::{Evaluation, Failed, own_solids};
use crate::cache::{Cache, Key};
use crate::error_geometry::KernelFailure;
use crate::message::{self, Blend, BlendRefusal};
use crate::{ErrorGeometry, inspect};

/// A body a feature changes in place (a chamfer, a fillet, a shell): its
/// solid as the features before it leave it, and that solid's topology.
pub(super) struct OwnBody<'a> {
    pub(super) body: BodyId,
    /// Its name, for messages.
    pub(super) name: &'a str,
    pub(super) solid: Arc<Solid>,
    /// The solid's cache key.
    pub(super) key: Key,
    pub(super) topology: Arc<Topology>,
}

impl<'a> OwnBody<'a> {
    /// The body `body` of `evaluation`, which must have a solid of its own
    /// ([`own_solids`]: one a join or a combine consumed fails, naming the
    /// body holding it).
    pub(super) fn take(
        document: &'a Document,
        body: BodyId,
        evaluation: &Evaluation,
        cache: &mut Cache,
    ) -> Result<Self, Failed> {
        own_solids(document, std::iter::once(body), evaluation)?;
        let name = document
            .body(body)
            .map_or("a body", |body| body.name.as_str());
        let made = (evaluation.bodies.iter())
            .find(|made| made.body == body)
            .expect("the body has a solid of its own");
        Ok(OwnBody {
            body,
            name,
            solid: Arc::clone(&made.solid),
            key: made.key,
            topology: inspect::topology(made, cache),
        })
    }

    /// Gives the body `result`, cached under `key`, in `evaluation`: it
    /// keeps its id.
    pub(super) fn replace(&self, evaluation: &mut Evaluation, result: Arc<Solid>, key: Key) {
        if let Some(made) = (evaluation.bodies.iter_mut()).find(|made| made.body == self.body) {
            made.solid = result;
            made.key = key;
        }
    }

    /// The chains of its topology `edges` are found as, in their order:
    /// one not found fails ("its edge 2 of 3 wasn't found").
    pub(super) fn edges(&self, edges: &[EdgeRef]) -> Result<Vec<u32>, Failed> {
        let count = edges.len();
        (edges.iter().enumerate())
            .map(|(i, edge)| {
                (self.topology.edge(&self.solid, edge.faces, edge.near))
                    .map_err(|_| message::blend_edge_not_found(i, count).into())
            })
            .collect()
    }
}

/// One chain to blend as it's worked out: which, its faces' name and
/// first face, and which of the feature's edges it comes from and
/// whether it was grown into, for the messages.
pub(super) struct Planned {
    pub(super) chain: u32,
    pub(super) name: u64,
    /// The region of its two that is its first face.
    pub(super) first: u32,
    pub(super) edge: usize,
    pub(super) grown: bool,
}

/// The chains `edges`, found as the chains `found` of `topology` (of
/// `solid`), come to, with tangent chains grown when `chains` and the
/// first faces the other way round when `flip`, as the module's docs
/// say.
pub(super) fn plan(
    solid: &Solid,
    topology: &Topology,
    edges: &[EdgeRef],
    found: &[u32],
    chains: bool,
    flip: bool,
) -> Vec<Planned> {
    let regions = topology.regions();
    let all = topology.chains();
    let key_of = |region: u32| regions[region as usize].key;
    let sides_of = |chain: u32| all[chain as usize].regions;
    // Each picked edge's first region.
    let firsts: Vec<u32> = (edges.iter().zip(found))
        .map(|(edge, &chain)| first_region(topology, sides_of(chain), edge, flip))
        .collect();
    let mut planned: Vec<Planned> = Vec::new();
    // Each chain taken, and how many chains taken have each pair: kept
    // as they go, so a long tangent chain costs its length once.
    let mut taken = vec![false; all.len()];
    let mut pairs: BTreeMap<[FaceKey; 2], u32> = BTreeMap::new();
    let mut add =
        |planned: &mut Vec<Planned>, chain: u32, pair: [FaceKey; 2], first, edge, grown| {
            if std::mem::replace(&mut taken[chain as usize], true) {
                return;
            }
            // Ordinals count the chains before it with the same pair.
            let ordinal = pairs.entry(pair).or_default();
            planned.push(Planned {
                chain,
                name: blend_edge(pair, *ordinal),
                first,
                edge,
                grown,
            });
            *ordinal += 1;
        };
    for (i, (edge, &chain)) in edges.iter().zip(found).enumerate() {
        add(&mut planned, chain, edge.faces, firsts[i], i, false);
    }
    if chains {
        let roots = topology.tangent_chains(solid);
        // The chains of each tangent chain, by its root, in index order.
        let mut members: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for (chain, &root) in roots.iter().enumerate() {
            members.entry(root).or_default().push(chain as u32);
        }
        let mut grown: Vec<bool> = vec![false; all.len()];
        for (i, &chain) in found.iter().enumerate() {
            let root = roots[chain as usize];
            // An earlier edge of the same tangent chain took it all.
            if std::mem::replace(&mut grown[root as usize], true) {
                continue;
            }
            let picked = sides_of(chain);
            let first = firsts[i];
            let second = if picked[0] == first {
                picked[1]
            } else {
                picked[0]
            };
            for &other in &members[&root] {
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

/// Why the kernel's chamfer or fillet, `blend`, of `own` with `count`
/// edges planned as `planned` gave no solid, in words, with what to
/// draw: the edge a refusal is about, or the failure's evidence.
pub(super) fn refused(
    blend: Blend,
    error: BlendError,
    own: &OwnBody,
    planned: &[Planned],
    tolerance: &Tolerance,
    count: usize,
) -> Failed {
    let about = |chain: u32| planned.iter().find(|p| p.chain == chain);
    let edge_failed = |chain: u32, why: BlendRefusal| {
        let which = about(chain).map(|p| (p.edge, p.grown));
        Failed {
            message: message::blend_refused(blend, why, which, count, own.name),
            geometry: chain_geometry(&own.solid, &own.topology, chain, tolerance),
        }
    };
    match error {
        BlendError::Flat { chain } => edge_failed(chain, BlendRefusal::Flat),
        BlendError::Folded { chain } => edge_failed(chain, BlendRefusal::Folded),
        BlendError::Mixed { chain } => edge_failed(chain, BlendRefusal::Mixed),
        BlendError::TooBig { chain } => edge_failed(chain, BlendRefusal::TooBig),
        BlendError::End { chain } => edge_failed(chain, BlendRefusal::End),
        BlendError::Corner { .. } => {
            message::blend_refused(blend, BlendRefusal::Corner, None, count, own.name).into()
        }
        BlendError::Failed(failure) => {
            let words = message::blending(blend, own.name, failure.error);
            let failure = KernelFailure::new(failure, tolerance);
            Failed::kernel(words, &failure, [&[own.body], &[]])
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

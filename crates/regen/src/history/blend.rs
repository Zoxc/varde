//! What chamfers and fillets share in the history: their body's solid
//! and topology, the edges found on it, the chains those come to (grown
//! along tangent chains where asked) and how they're named, and the
//! kernel's refusals worded with the edge drawn. Their body is taken as
//! `in_place` has it.
//!
//! The edges are found on the body's topology (the one drawing it keeps,
//! [`InPlace::topology`]) by their faces' keys and points, as a scale's
//! edge is; one not found fails the feature ("its edge wasn't found",
//! "its edge 2 of 3 wasn't found"). With tangent chains on, each edge
//! takes in the chains running on smoothly from it
//! ([`Topology::tangent_chains`]: purely the topology and the curves'
//! end tangents, so it's done here rather than in the kernel). A chain
//! is handed over once: the edges picked first, in the feature's order,
//! then those grown into, each taken by the first edge reaching it.
//!
//! The feature's faces are found as regions ("its face 2 of 3 wasn't
//! found") and stand for the edges around them. An edge the feature
//! names that runs along one of its faces is left out (an exclusion),
//! and no tangent chain grows into it; any other edge it names is
//! picked, then each face's edges in index order, the face their first
//! face (the region across with `flip`), named by their regions' keys.
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

use varde_document::{EdgeRef, FaceRef};
use varde_kernel::mesh::FaceKey;
use varde_kernel::topology::blend_edge;
use varde_kernel::{BlendError, Evidence, Solid, Tolerance, Topology};

use super::Failed;
use super::in_place::InPlace;
use crate::ErrorGeometry;
use crate::error_geometry::KernelFailure;
use crate::message::{self, Blend, BlendRefusal, BlendSource};

/// The chains of `own`'s topology `edges` are found as, in their order:
/// one not found fails ("its edge 2 of 3 wasn't found").
pub(super) fn find_edges(own: &InPlace, edges: &[EdgeRef]) -> Result<Vec<u32>, Failed> {
    let count = edges.len();
    (edges.iter().enumerate())
        .map(|(i, edge)| {
            (own.topology.edge(&own.solid, edge.faces, edge.near))
                .map_err(|_| message::blend_edge_not_found(i, count).into())
        })
        .collect()
}

/// The regions of `own`'s topology `faces` are found as, in their order:
/// one not found fails ("its face 2 of 3 wasn't found").
pub(super) fn find_faces(own: &InPlace, faces: &[FaceRef]) -> Result<Vec<u32>, Failed> {
    let count = faces.len();
    (faces.iter().enumerate())
        .map(|(i, face)| {
            (own.topology.face(&own.solid, &face.key, face.near))
                .map_err(|_| message::blend_face_not_found(i, count).into())
        })
        .collect()
}

/// One chain to blend as it's worked out: which, its faces' name and
/// first face, and which of the feature's edges or faces it comes from
/// and whether it was grown into, for the messages.
pub(super) struct Planned {
    pub(super) chain: u32,
    pub(super) name: u64,
    /// The region of its two that is its first face.
    pub(super) first: u32,
    pub(super) source: BlendSource,
    pub(super) grown: bool,
}

/// A chain blended for what the feature names, before tangent chains
/// grow from it: its pair of keys and first region.
struct Seed {
    chain: u32,
    pair: [FaceKey; 2],
    first: u32,
    source: BlendSource,
}

/// The chains `edges` and the faces' edges come to, `edges` found as the
/// chains `found` of `topology` (of `solid`) and the faces as the
/// regions `faces`, with tangent chains grown when `chains` and the
/// first faces the other way round when `flip`, as the module's docs
/// say.
pub(super) fn plan(
    solid: &Solid,
    topology: &Topology,
    edges: &[EdgeRef],
    found: &[u32],
    faces: &[u32],
    chains: bool,
    flip: bool,
) -> Vec<Planned> {
    let regions = topology.regions();
    let all = topology.chains();
    let key_of = |region: u32| regions[region as usize].key;
    let sides_of = |chain: u32| all[chain as usize].regions;
    let other_side = |chain: u32, side: u32| {
        let sides = sides_of(chain);
        if sides[0] == side { sides[1] } else { sides[0] }
    };
    // An edge named around a face named is left out, and never grown
    // into.
    let mut taken = vec![false; all.len()];
    let on_face = |chain: u32| sides_of(chain).iter().any(|side| faces.contains(side));
    let mut seeds: Vec<Seed> = Vec::new();
    for (i, (edge, &chain)) in edges.iter().zip(found).enumerate() {
        if on_face(chain) {
            taken[chain as usize] = true;
        } else {
            seeds.push(Seed {
                chain,
                pair: edge.faces,
                first: first_region(topology, sides_of(chain), edge, flip),
                source: BlendSource::Edge(i),
            });
        }
    }
    // Each face's edges in index order, its first face the face (the
    // other side with `flip`).
    for (i, &face) in faces.iter().enumerate() {
        for (chain, sides) in all
            .iter()
            .enumerate()
            .map(|(c, chain)| (c as u32, chain.regions))
        {
            if !sides.contains(&face) || taken[chain as usize] {
                continue;
            }
            let mut pair = sides.map(key_of);
            pair.sort();
            seeds.push(Seed {
                chain,
                pair,
                first: if flip { other_side(chain, face) } else { face },
                source: BlendSource::Face(i),
            });
        }
    }
    let mut planned: Vec<Planned> = Vec::new();
    // How many chains taken have each pair: kept as they go, so a long
    // tangent chain costs its length once.
    let mut pairs: BTreeMap<[FaceKey; 2], u32> = BTreeMap::new();
    let mut add =
        |planned: &mut Vec<Planned>, chain: u32, pair: [FaceKey; 2], first, source, grown| {
            if std::mem::replace(&mut taken[chain as usize], true) {
                return;
            }
            // Ordinals count the chains before it with the same pair.
            let ordinal = pairs.entry(pair).or_default();
            planned.push(Planned {
                chain,
                name: blend_edge(pair, *ordinal),
                first,
                source,
                grown,
            });
            *ordinal += 1;
        };
    for seed in &seeds {
        add(
            &mut planned,
            seed.chain,
            seed.pair,
            seed.first,
            seed.source,
            false,
        );
    }
    if chains {
        let roots = topology.tangent_chains(solid);
        // The chains of each tangent chain, by its root, in index order.
        let mut members: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for (chain, &root) in roots.iter().enumerate() {
            members.entry(root).or_default().push(chain as u32);
        }
        let mut grown: Vec<bool> = vec![false; all.len()];
        for seed in &seeds {
            let root = roots[seed.chain as usize];
            // An earlier seed of the same tangent chain took it all.
            if std::mem::replace(&mut grown[root as usize], true) {
                continue;
            }
            let first = seed.first;
            let second = other_side(seed.chain, first);
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
                add(&mut planned, other, pair, other_first, seed.source, true);
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

/// Why the kernel's chamfer or fillet, `blend`, of `own` naming `counts`
/// edges and faces, planned as `planned`, gave no solid, in words, with what to
/// draw: the edge a refusal is about, or the failure's evidence.
pub(super) fn refused(
    blend: Blend,
    error: BlendError,
    own: &InPlace,
    planned: &[Planned],
    tolerance: &Tolerance,
    counts: [usize; 2],
) -> Failed {
    let about = |chain: u32| planned.iter().find(|p| p.chain == chain);
    let edge_failed = |chain: u32, why: BlendRefusal| {
        let which = about(chain).map(|p| (p.source, p.grown));
        Failed {
            message: message::blend_refused(blend, why, which, counts, own.name),
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
            message::blend_refused(blend, BlendRefusal::Corner, None, counts, own.name).into()
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

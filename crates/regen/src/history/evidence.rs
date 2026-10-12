//! What regen's own failures show of a face or an edge, as evidence
//! drawn by value, and the corners of a box.

use std::sync::Arc;

use glam::DVec3;
use varde_kernel_compat::{Solid, Tolerance};

use crate::error_geometry::ErrorGeometry;

/// What a face that can't be sketched on shows: `region` of `solid` (the
/// face found), its triangles as patches, the first
/// [`MAX_EVIDENCE`](varde_kernel_compat::MAX_EVIDENCE)`.patches` of them (marked
/// truncated past that), drawn at the
/// [`Display`](varde_kernel_compat::Display) of `tolerance`. By value, not
/// resolved through the picking tables as an operand face is: the solid
/// is the body as the features before the sketch leave it, which later
/// ones may change. Bounded as the kernel's evidence is: taking each
/// patch costs a lookup, and drawing them is what drawing a kernel
/// failure's patches costs, within the same caps
/// ([`ErrorGeometry::MAX_VERTICES`] and the others), once per placement
/// worked out (the cache keeps it).
pub(crate) fn face_geometry(
    solid: &Solid,
    region: &varde_kernel_compat::topology::Region,
    tolerance: &Tolerance,
) -> Option<Arc<ErrorGeometry>> {
    let mut evidence = varde_kernel_compat::Evidence::default();
    add_region(&mut evidence, solid, region);
    ErrorGeometry::of_evidence(&evidence, tolerance)
}

/// Adds the patches of `region` (of `solid`) to `evidence`, those of
/// triangles `solid`'s mesh has.
pub(crate) fn add_region(
    evidence: &mut varde_kernel_compat::Evidence,
    solid: &Solid,
    region: &varde_kernel_compat::topology::Region,
) {
    let mesh = solid.mesh();
    let tris = mesh.tris().len();
    evidence.add_patches(
        (region.tris.iter())
            .filter(|&&tri| (tri as usize) < tris)
            .map(|&tri| mesh.patch(tri as usize)),
    );
}

/// Adds the curves of `chain` (of `solid`) to `evidence`, those of
/// halfedges `solid`'s mesh has.
pub(crate) fn add_chain(
    evidence: &mut varde_kernel_compat::Evidence,
    solid: &Solid,
    chain: &varde_kernel_compat::topology::Chain,
) {
    let mesh = solid.mesh();
    let tris = mesh.tris().len();
    evidence.add_curves(
        (chain.halfedges.iter())
            .filter(|&&h| (h as usize) / 3 < tris)
            .map(|&h| mesh.curve(h)),
    );
}

/// What an edge that isn't the shape a feature needs shows: `chain`'s
/// curves on `solid`, drawn at the [`Display`](varde_kernel_compat::Display) of
/// `tolerance` (the first
/// [`MAX_EVIDENCE`](varde_kernel_compat::MAX_EVIDENCE)`.curves`), by value as a
/// face that isn't flat is ([`face_geometry`]).
pub(crate) fn chain_curves(
    solid: &Solid,
    chain: &varde_kernel_compat::topology::Chain,
    tolerance: &Tolerance,
) -> Option<Arc<ErrorGeometry>> {
    let mut evidence = varde_kernel_compat::Evidence::default();
    add_chain(&mut evidence, solid, chain);
    ErrorGeometry::of_evidence(&evidence, tolerance)
}

/// The eight corners of `bounds`.
pub(crate) fn corners(bounds: &varde_kernel_compat::patch::Bounds3) -> [DVec3; 8] {
    std::array::from_fn(|corner| {
        let pick = |bit: usize, min: f64, max: f64| if corner & bit == 0 { min } else { max };
        DVec3::new(
            pick(1, bounds.min.x, bounds.max.x),
            pick(2, bounds.min.y, bounds.max.y),
            pick(4, bounds.min.z, bounds.max.z),
        )
    })
}

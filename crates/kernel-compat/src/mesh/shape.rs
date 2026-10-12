//! The shape of triangles in a plane, for the triangulators that
//! refine plane regions with points at circumcentres: the extrude's caps
//! (`extrude/cap/quality.rs`), the boolean's cut faces
//! (`boolean/triangulate.rs`) and the plane faces the boolean's clean-up
//! refines as a whole (`boolean/cleanup/quality.rs`). Each keeps its own
//! queue, walk and clearance (the caps insert into a constrained Delaunay
//! triangulation, the cut faces split and flip their own triangles under
//! curved-corner rules, the clean-up walks and flips on the soup, where a
//! side between two plane faces can be halved in both); the bound and the
//! circumcentre are one.

use glam::DVec2;

/// The sine of the narrowest angle a refined triangle keeps: sin 5°.
pub(crate) const SIN_SHAPE: f64 = 0.087_155_742_747_658_17;

/// The circumcentre of the triangle `a`, `b`, `c`, less `a`: the
/// circumradius is its length. Plain `+ − × ÷` from `a`, so the same
/// bits everywhere; not finite where the triangle is flat.
pub(crate) fn circumcentre_from(a: DVec2, b: DVec2, c: DVec2) -> DVec2 {
    let (ba, ca) = (b - a, c - a);
    let (bb, cc) = (ba.length_squared(), ca.length_squared());
    let d = 2.0 * ba.perp_dot(ca);
    DVec2::new(ca.y * bb - ba.y * cc, ba.x * cc - ca.x * bb) / d
}

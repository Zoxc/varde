//! The stand-ins for the kernel operations that aren't built yet, one
//! file each, and the box several of them take.

pub(super) mod chamfer;
pub(super) mod face_draft;
pub(super) mod fillet;
pub(super) mod loft;
pub(super) mod offset_face;
pub(super) mod shell;
pub(super) mod split;
pub(super) mod sweep;
pub(super) mod taper;

use glam::DVec3;
use varde_kernel_compat::mesh::{Edge, Face, Form, Mesh};
use varde_kernel_compat::{Solid, Topology};

/// A box's least and greatest corners, and each region's side of it
/// ([`box_sides`]).
pub(super) type BoxSides = (DVec3, DVec3, Vec<(usize, usize)>);

/// The box a stand-in for the kernel takes: `solid` must be a box along
/// the world's axes (six planar faces square to them, its volume its
/// box's). Its least and greatest corners, and each region's side of
/// it: the axis, and 0 at the low end, 1 at the high. `None` for
/// anything else.
pub(super) fn box_sides(solid: &Solid, topology: &Topology) -> Option<BoxSides> {
    let bounds = solid.bounds3()?;
    let (min, max) = (bounds.min, bounds.max);
    let size = max - min;
    let volume = size.x * size.y * size.z;
    if topology.regions().len() != 6 || (solid.volume() - volume).abs() > 1e-9 * volume {
        return None;
    }
    let mut sides = Vec::with_capacity(6);
    for region in topology.regions() {
        let Form::Plane { n, .. } = *crate::picking::region_form(solid, region) else {
            return None;
        };
        let n = n.normalize();
        let axis = n.abs().max_position();
        if (n.abs()[axis] - 1.0).abs() > 1e-12 {
            return None;
        }
        sides.push((axis, usize::from(n[axis] > 0.0)));
    }
    Some((min, max, sides))
}

/// A straight, open, convex edge between two planar faces
/// ([`convex_edge`]).
pub(super) struct ConvexEdge {
    /// The edge's start and end.
    pub(super) a: DVec3,
    pub(super) b: DVec3,
    /// The unit direction from `a` to `b`, and the edge's length.
    pub(super) t: DVec3,
    pub(super) length: f64,
    /// Into each face from the edge, square to it.
    pub(super) into: [DVec3; 2],
    /// Each face's unit outward normal.
    pub(super) normal: [DVec3; 2],
    /// Each face's region.
    pub(super) regions: [u32; 2],
}

/// Chain `chain` of `topology` as the edge a chamfer or fillet stand-in
/// takes: open, of nonzero length, between two planar faces, convex
/// (each face behind the other's plane). Its ends are its first and
/// last curves' ends, so a straight chain is assumed. `None` for
/// anything else.
pub(super) fn convex_edge(solid: &Solid, topology: &Topology, chain: u32) -> Option<ConvexEdge> {
    let mesh = solid.mesh();
    let chain = topology.chains().get(chain as usize)?;
    if chain.closed {
        return None;
    }
    let a = mesh.curve(*chain.halfedges.first()?).p0;
    let b = mesh.curve(*chain.halfedges.last()?).p1;
    let length = (b - a).length();
    if !(length.is_finite() && length > 0.0) {
        return None;
    }
    let t = (b - a) / length;
    let mut into = [DVec3::ZERO; 2];
    let mut normal = [DVec3::ZERO; 2];
    for (side, &region) in chain.regions.iter().enumerate() {
        let region = &topology.regions()[region as usize];
        let Form::Plane { n, .. } = *crate::picking::region_form(solid, region) else {
            return None;
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
        return None;
    }
    Some(ConvexEdge {
        a,
        b,
        t,
        length,
        into,
        normal,
        regions: chain.regions,
    })
}

/// Whether every corner a stand-in moved is finite and within
/// [`MAX_COORD`](varde_kernel_compat::MAX_COORD), as a solid's must be.
pub(super) fn in_reach(verts: &[DVec3]) -> bool {
    let reach = f64::from(varde_kernel_compat::MAX_COORD);
    (verts.iter()).all(|v| v.is_finite() && v.abs().max_element() <= reach)
}

/// `mesh` with its vertices moved to `verts` (one each, in order) and
/// its faces replaced by `faces`: every edge rebuilt straight between
/// its moved ends, the triangles and aliases kept.
pub(super) fn with_moved_corners(mesh: &Mesh, verts: Vec<DVec3>, faces: Vec<Face>) -> Mesh {
    let mut edges = mesh.edges().to_vec();
    for tri in mesh.tris() {
        for (i, half) in tri.halfedges.iter().enumerate() {
            let end = tri.halfedges[(i + 1) % 3].start;
            edges[half.edge as usize] =
                Edge::straight(verts[half.start as usize], verts[end as usize]);
        }
    }
    Mesh::from_parts(verts, edges, mesh.tris().to_vec(), faces)
        .with_aliases(mesh.aliases().to_vec())
}

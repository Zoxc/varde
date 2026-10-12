//! About how many bytes a kept result holds, for the cache's budget:
//! [`HeapSize`] and its impls for the kernel's and the sketch's types.
//! Regen's own types implement it beside their private fields. Saturating,
//! since the sizes come from what the user drew.

use std::mem::size_of_val;

use varde_kernel_compat::{MeshUpload, RenderMesh, Solid, Topology};
use varde_sketch::{Curve, LinkShape, Profiles, Sketch};

/// What a value holds beyond itself: its lists' lengths, not their
/// capacities.
pub(crate) trait HeapSize {
    /// The bytes it holds behind pointers, not counting
    /// `size_of::<Self>()`.
    fn heap_bytes(&self) -> usize;
}

/// `value`'s size with what it holds: for a value behind a pointer.
pub(crate) fn total<T: HeapSize>(value: &T) -> usize {
    size_of_val(value).saturating_add(value.heap_bytes())
}

impl HeapSize for LinkShape {
    /// Its lists, and its splines' points.
    fn heap_bytes(&self) -> usize {
        (size_of_val(&self.points[..]))
            .saturating_add(size_of_val(&self.curves[..]))
            .saturating_add(splines_bytes(&self.curves))
    }
}

impl HeapSize for Sketch {
    /// Its lists' entries, and a spline's points.
    fn heap_bytes(&self) -> usize {
        (size_of_val(&self.points[..]))
            .saturating_add(size_of_val(&self.curves[..]))
            .saturating_add(size_of_val(&self.constraints[..]))
            .saturating_add(size_of_val(&self.dimensions[..]).saturating_mul(2))
            .saturating_add(size_of_val(&self.links[..]))
            .saturating_add(splines_bytes(self.curves.iter().map(|entry| &entry.curve)))
    }
}

/// What the splines among `curves` hold beyond their entries: their
/// points and knots.
fn splines_bytes<'a>(curves: impl IntoIterator<Item = &'a Curve>) -> usize {
    (curves.into_iter()).fold(0usize, |sum, curve| match curve {
        Curve::Spline(spline) => sum
            .saturating_add(size_of_val(&spline.points[..]))
            .saturating_add(size_of_val(&spline.knots[..])),
        _ => sum,
    })
}

impl HeapSize for Solid {
    fn heap_bytes(&self) -> usize {
        let mesh = self.mesh();
        (size_of_val(mesh.verts()))
            .saturating_add(size_of_val(mesh.edges()))
            .saturating_add(size_of_val(mesh.tris()))
            .saturating_add(size_of_val(mesh.faces()))
    }
}

impl HeapSize for Topology {
    fn heap_bytes(&self) -> usize {
        let mut bytes = (size_of_val(self.regions()))
            .saturating_add(size_of_val(self.chains()))
            .saturating_add(size_of_val(self.corners()))
            .saturating_add(self.triangles().saturating_mul(size_of::<u32>()));
        for region in self.regions() {
            bytes = (bytes.saturating_add(size_of_val(&region.tris[..])))
                .saturating_add(size_of_val(&region.aliases[..]));
        }
        for chain in self.chains() {
            bytes = bytes.saturating_add(size_of_val(&chain.halfedges[..]));
        }
        for corner in self.corners() {
            bytes = bytes.saturating_add(size_of_val(&corner.regions[..]));
        }
        bytes
    }
}

impl HeapSize for RenderMesh {
    /// Its lists; not its upload, which a scene's mesh alone has built.
    fn heap_bytes(&self) -> usize {
        (size_of_val(self.positions()))
            .saturating_add(size_of_val(self.normals()))
            .saturating_add(size_of_val(self.indices()))
            .saturating_add(size_of_val(self.face_ends()))
            .saturating_add(size_of_val(self.edge_vertices()))
            .saturating_add(size_of_val(self.edge_ends()))
            .saturating_add(size_of_val(self.edge_faces()))
            .saturating_add(size_of_val(self.corners()))
            .saturating_add(size_of_val(self.edge_corners()))
            .saturating_add(size_of_val(self.wire_vertices()))
            .saturating_add(size_of_val(self.wire_ends()))
            .saturating_add(size_of_val(self.part_ends()))
    }
}

impl HeapSize for MeshUpload {
    fn heap_bytes(&self) -> usize {
        (size_of_val(self.points()))
            .saturating_add(size_of_val(self.parts()))
            .saturating_add(size_of_val(self.bounds()))
    }
}

impl HeapSize for Profiles {
    fn heap_bytes(&self) -> usize {
        let mut bytes = (size_of_val(&self.regions[..]))
            .saturating_add(size_of_val(&self.vertices[..]))
            .saturating_add(size_of_val(&self.open_ends[..]));
        for region in &self.regions {
            bytes = bytes.saturating_add(size_of_val(&region.outer[..]));
            for hole in &region.holes {
                bytes = (bytes.saturating_add(size_of_val(hole)))
                    .saturating_add(size_of_val(&hole[..]));
            }
            for outline in &region.outline {
                bytes = (bytes.saturating_add(size_of_val(outline)))
                    .saturating_add(size_of_val(&outline[..]));
            }
        }
        bytes
    }
}

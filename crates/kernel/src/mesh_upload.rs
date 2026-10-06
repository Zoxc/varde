//! What the renderer uploads of a [`RenderMesh`] beyond its own vectors:
//! the stream of points its feature edges and wires are drawn from, where
//! each part's are in it, and each part's bounds. Pure CPU work, so the
//! regeneration lane builds it ([`RenderMesh::upload`]) and the UI thread
//! only copies it to the GPU.

use std::ops::Range;

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use crate::{Aabb, RenderMesh};

/// A point of the stream the feature edges are drawn from (see
/// [`EdgeStream`] and `agents/viewport.md`). The renderer binds the stream
/// to four vertex buffer slots a point apart, so the instance drawing the
/// segment from point `i + 1` to `i + 2` sees the points either side too.
/// See `edge_segment` in the renderer's shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct EdgePoint {
    pub position: [f32; 3],
    /// How far along its polyline it is, in world units.
    pub along: f32,
    /// Which polyline it's on.
    pub edge: u32,
}

/// The edge of the points at the ends of the stream: no polyline's.
pub const NO_EDGE: u32 = u32::MAX;

/// Set in [`EdgePoint::edge`] for a point that is only a neighbour of its
/// edge's segments, where a closed polyline joins itself. As
/// `NEIGHBOUR_ONLY` in the shader.
pub const NEIGHBOUR_ONLY: u32 = 1 << 31;

/// Set in [`EdgePoint::edge`] for a crease's points: an edge with one face
/// on both sides. As `CREASE` in the shader.
pub const CREASE: u32 = 1 << 30;
const _: () = assert!(RenderMesh::MAX_EDGE_POLYLINES <= CREASE as usize);

/// What the renderer uploads of a mesh beyond its own vectors, see the
/// module docs: [`MeshUpload::new`] builds it, [`MeshUpload::from_parts`]
/// checks one that came from elsewhere.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshUpload {
    points: Vec<EdgePoint>,
    parts: Vec<[Range<u32>; 2]>,
    bounds: Vec<Option<Aabb>>,
}

/// Why parts don't make a [`MeshUpload`] of a mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadError {
    /// There isn't one range pair and one box per part of the mesh, or
    /// more points than the mesh's edges and wires can make.
    Lengths,
    /// A point's position is past [`RenderMesh::MAX_POSITION`], or its
    /// edge is past the mesh's edges and wires.
    Points,
    /// The stream doesn't start and end with a point of no edge, or a
    /// part's ranges aren't in order between them.
    Ranges,
    /// A part's box isn't finite or its corners are out of order.
    Bounds,
}

impl std::fmt::Display for UploadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            UploadError::Lengths => "the edge stream doesn't go with the mesh",
            UploadError::Points => "the edge stream's points are out of range",
            UploadError::Ranges => "the edge stream's part ranges are out of order",
            UploadError::Bounds => "a part's bounds aren't a box",
        })
    }
}

impl std::error::Error for UploadError {}

impl MeshUpload {
    /// The most points a mesh's stream can have: one and a half per edge
    /// and wire vertex (closed polylines get two more), plus one at each
    /// end.
    pub const MAX_POINTS: usize = RenderMesh::MAX_EDGE_POINTS / 2 * 3 + 2;

    /// The most points `mesh`'s stream can have: one and a half per edge
    /// and wire vertex and one at each end, within
    /// [`MeshUpload::MAX_POINTS`]; none without edges or wires.
    pub fn most_points(mesh: &RenderMesh) -> usize {
        let vertices = (mesh.edge_vertices().len()).saturating_add(mesh.wire_vertices().len());
        if vertices == 0 {
            return 0;
        }
        (vertices.saturating_mul(3) / 2)
            .saturating_add(2)
            .min(Self::MAX_POINTS)
    }

    /// `mesh`'s stream: its feature edges, then its wires, creases and
    /// wires marked [`CREASE`], a wire numbered after every edge; none if
    /// it has neither. And where each part's edges' and wires' points are
    /// in it, and its triangles' bounds.
    pub fn new(mesh: &RenderMesh) -> MeshUpload {
        let bounds = mesh
            .parts()
            .map(|part| bounds_of(mesh.positions(), &mesh.indices()[part.indices]))
            .collect();
        if mesh.edge_vertices().is_empty() && mesh.wire_vertices().is_empty() {
            return MeshUpload {
                points: Vec::new(),
                parts: Vec::new(),
                bounds,
            };
        }
        let points = (mesh.edge_vertices().len()).saturating_add(mesh.wire_vertices().len());
        let mut stream = EdgeStream::with_capacity(points);
        // The kernel bounds the edges and wires together well within `u32`,
        // below `CREASE`.
        let to_u32 = |n: usize| u32::try_from(n).expect("kernel bounds edges");
        let mut edge_points = Vec::with_capacity(mesh.part_ends().len());
        let mut polylines = mesh.polylines().zip(mesh.edge_faces());
        for part in mesh.parts() {
            let start = stream.len();
            for (edge, (polyline, [a, b])) in part.edges.zip(polylines.by_ref()) {
                let id = if a == b {
                    to_u32(edge) | CREASE
                } else {
                    to_u32(edge)
                };
                stream.push(id, mesh.positions(), polyline);
            }
            edge_points.push(start..stream.len());
        }
        let edges = to_u32(mesh.edge_count());
        let mut wires = mesh.wires();
        let parts = (mesh.parts().zip(edge_points))
            .map(|(part, edge_points)| {
                let start = stream.len();
                for (wire, polyline) in part.wires.zip(wires.by_ref()) {
                    stream.push((edges + to_u32(wire)) | CREASE, mesh.positions(), polyline);
                }
                [edge_points, start..stream.len()]
            })
            .collect();
        MeshUpload {
            points: stream.finish(),
            parts,
            bounds,
        }
    }

    /// An upload of `mesh` made of these parts, as [`MeshUpload::points`],
    /// [`MeshUpload::parts`] and [`MeshUpload::bounds`] give them, if they
    /// could be one: points within the mesh's bounds and edges, ranges in
    /// order within the stream, boxes finite. Not that they're the very
    /// ones [`MeshUpload::new`] makes: they only draw wrong.
    pub fn from_parts(
        mesh: &RenderMesh,
        points: Vec<EdgePoint>,
        parts: Vec<[Range<u32>; 2]>,
        bounds: Vec<Option<Aabb>>,
    ) -> Result<MeshUpload, UploadError> {
        let part_count = mesh.part_ends().len();
        let no_lines = mesh.edge_vertices().is_empty() && mesh.wire_vertices().is_empty();
        if bounds.len() != part_count
            || parts.len() != if no_lines { 0 } else { part_count }
            || points.len() > Self::most_points(mesh)
        {
            return Err(UploadError::Lengths);
        }
        let lines = mesh.edge_count().saturating_add(mesh.wire_ends().len());
        let point_ok = |point: &EdgePoint| {
            let edge = point.edge & !(NEIGHBOUR_ONLY | CREASE);
            point
                .position
                .iter()
                .all(|v| v.abs() <= RenderMesh::MAX_POSITION)
                && point.along.is_finite()
                && (point.edge == NO_EDGE || (edge as usize) < lines)
        };
        if !points.iter().all(point_ok) {
            return Err(UploadError::Points);
        }
        // A stream with lines starts and ends with a point of no edge,
        // and each range is in order between them, edges' then wires':
        // the renderer reads a point either side of a range.
        let ends = |point: Option<&EdgePoint>| point.is_some_and(|p| p.edge == NO_EDGE);
        if !no_lines && !(ends(points.first()) && ends(points.last()) && points.len() >= 2) {
            return Err(UploadError::Ranges);
        }
        let last = points.len().saturating_sub(1) as u64;
        let mut at = 1u32;
        for range in
            (parts.iter().map(|[edges, _]| edges)).chain(parts.iter().map(|[_, wires]| wires))
        {
            if range.start < at || range.end < range.start || u64::from(range.end) > last {
                return Err(UploadError::Ranges);
            }
            at = range.end;
        }
        let box_ok = |aabb: &Option<Aabb>| {
            aabb.is_none_or(|Aabb { min, max }| {
                min.is_finite() && max.is_finite() && min.cmple(max).all()
            })
        };
        if !bounds.iter().all(box_ok) {
            return Err(UploadError::Bounds);
        }
        Ok(MeshUpload {
            points,
            parts,
            bounds,
        })
    }

    /// The stream of the mesh's edges' and wires' points, empty if it has
    /// neither.
    pub fn points(&self) -> &[EdgePoint] {
        &self.points
    }

    /// Where each part's edges' and wires' points are in the stream, one
    /// pair per part; none if the stream is empty.
    pub fn parts(&self) -> &[[Range<u32>; 2]] {
        &self.parts
    }

    /// Each part's triangles' bounds, `None` for a part without any.
    pub fn bounds(&self) -> &[Option<Aabb>] {
        &self.bounds
    }
}

/// The bounds of the positions `indices` refer to, or `None` if there are
/// none.
fn bounds_of(positions: &[[f32; 3]], indices: &[u32]) -> Option<Aabb> {
    let mut points = indices.iter().map(|&i| Vec3::from(positions[i as usize]));
    let first = points.next()?;
    let (min, max) = points.fold((first, first), |(min, max), p| (min.min(p), max.max(p)));
    Some(Aabb { min, max })
}

/// An [`EdgePoint`] stream being built: polylines one after another, from
/// a point of no edge, which ends it too.
pub struct EdgeStream {
    points: Vec<EdgePoint>,
    /// A polyline's points, repeats in a row left out: a segment of no
    /// length between two others would keep them from joining.
    kept: Vec<[f32; 3]>,
}

impl EdgeStream {
    /// A stream with room for `points` points of polylines.
    pub fn with_capacity(points: usize) -> EdgeStream {
        let mut stream = EdgeStream {
            points: Vec::with_capacity(points.saturating_add(2)),
            kept: Vec::new(),
        };
        stream.separate();
        stream
    }

    /// Appends the polyline through `polyline`'s vertices of `positions`
    /// as edge `edge`, below [`NEIGHBOUR_ONLY`].
    pub fn push(&mut self, edge: u32, positions: &[[f32; 3]], polyline: &[u32]) {
        self.push_points(
            edge,
            polyline.iter().map(|&vertex| positions[vertex as usize]),
        );
    }

    /// Appends the polyline through `polyline` as edge `edge`, likewise.
    pub fn push_points(&mut self, edge: u32, polyline: impl IntoIterator<Item = [f32; 3]>) {
        let (points, kept) = (&mut self.points, &mut self.kept);
        kept.clear();
        for position in polyline {
            if kept.last() != Some(&position) {
                kept.push(position);
            }
        }
        let neighbour = |position| EdgePoint {
            position,
            along: 0.0,
            edge: edge | NEIGHBOUR_ONLY,
        };
        // Closed, round three segments or more; two would be one there and
        // back, joined at its turns already.
        let closed = kept.len() >= 4 && kept.first() == kept.last();
        if closed {
            points.push(neighbour(kept[kept.len() - 2]));
        }
        // Summed in `f64`, so a long polyline of short segments doesn't
        // drift. Positions are bounded, so it stays finite.
        let mut along = 0.0f64;
        for (i, &position) in kept.iter().enumerate() {
            if let Some(&last) = i.checked_sub(1).and_then(|i| kept.get(i)) {
                along += f64::from(Vec3::from(position).distance(Vec3::from(last)));
            }
            points.push(EdgePoint {
                position,
                along: along as f32,
                edge,
            });
        }
        if closed {
            points.push(neighbour(kept[1]));
        }
        // One left of the polyline, all its points the same, is a dot.
        if let [point] = kept[..] {
            points.push(EdgePoint {
                position: point,
                along: 0.0,
                edge,
            });
        }
    }

    /// Appends a point of no edge, which no segment joins.
    pub fn separate(&mut self) {
        self.points.push(EdgePoint {
            position: [0.0; 3],
            along: 0.0,
            edge: NO_EDGE,
        });
    }

    /// How many points it has.
    pub fn len(&self) -> u32 {
        // The kernel's limits keep the stream well within `u32`.
        u32::try_from(self.points.len()).expect("kernel bounds edges")
    }

    /// Whether it has no points, which it never has: it starts with a
    /// point of no edge.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// The stream, ended by a point of no edge.
    pub fn finish(mut self) -> Vec<EdgePoint> {
        self.separate();
        self.points
    }
}

#[cfg(test)]
mod tests;

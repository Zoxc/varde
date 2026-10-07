//! What of the model is hovered and selected, drawn over it: its edges
//! and vertices. The faces are drawn by their index ranges instead, see
//! [`Frame::hovered_faces`](crate::Frame::hovered_faces).

use std::ops::Range;

use bytemuck::{Pod, Zeroable};
use varde_kernel::RenderMesh;

use varde_kernel::{EdgePoint, EdgeStream};

/// The edges and vertices of a [`Frame`](crate::Frame)'s mesh that are
/// hovered or selected, by their ids in it: an edge by its polyline
/// ([`RenderMesh::edge_ends`]), a vertex by its corner
/// ([`RenderMesh::corners`]). Ids the mesh hasn't are left out. The
/// renderer uploads it again only when it's another `Arc`, so build it
/// only when the hover or the selection changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Highlights {
    /// The edges outlined, as hovered: drawn again
    /// [`HOVERED_EDGE_WIDTH`](crate::HOVERED_EDGE_WIDTH) wide in the edges'
    /// colour, with a rim [`HOVER_RIM`](crate::HOVER_RIM) wide outside
    /// them in [`Colors::hover_outline`](crate::Colors::hover_outline).
    pub outlined: Vec<u32>,
    /// The edges selected: drawn again in
    /// [`Colors::selected`](crate::Colors::selected),
    /// [`SELECTED_EDGE_WIDTH`](crate::SELECTED_EDGE_WIDTH) wide, over the
    /// outline.
    pub selected_edges: Vec<u32>,
    /// The edges in the second colour (the measure tool's B): drawn as
    /// the selected are, in [`Colors::second`](crate::Colors::second).
    pub second_edges: Vec<u32>,
    /// The vertices, drawn as discs of radius
    /// [`VERTEX_RADIUS`](crate::VERTEX_RADIUS) within a rim if hovered or
    /// selected.
    pub vertices: Vec<Vertex>,
}

/// A vertex of [`Highlights::vertices`]: its corner, and whether it's
/// hovered and selected. Hovered, it's in the edges' colour within a rim
/// [`HOVER_RIM`](crate::HOVER_RIM) wide in
/// [`Colors::hover_outline`](crate::Colors::hover_outline); selected, it's
/// filled with [`Colors::selected`](crate::Colors::selected), within a
/// pixel's rim in the edges' colour unless it's hovered too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vertex {
    pub corner: u32,
    pub hovered: bool,
    pub selected: bool,
}

/// Set in [`VertexInstance::flags`] for a hovered vertex, and a selected
/// one. As `HOVERED` and `SELECTED` in the shader.
const HOVERED: u32 = 1;
const SELECTED: u32 = 2;

/// A vertex as the GPU takes it, an instance each: see `vs_vertex`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(crate) struct VertexInstance {
    pub(crate) position: [f32; 3],
    /// [`HOVERED`] and [`SELECTED`].
    pub(crate) flags: u32,
}

/// [`Highlights`] in `mesh` as the renderer draws them: the edges as an
/// [`EdgePoint`] stream, the outlined ones' points and the selected ones',
/// apart, and a [`VertexInstance`] per vertex.
pub(crate) struct Built {
    pub(crate) edges: Vec<EdgePoint>,
    pub(crate) outlined: Range<u32>,
    pub(crate) selected: Range<u32>,
    pub(crate) second: Range<u32>,
    pub(crate) face_outlines: Vec<FaceOutline>,
    pub(crate) vertices: Vec<VertexInstance>,
}

/// A selected face's boundary edges in [`Built::edges`], drawn where the
/// model hides them in the colour its stripes are: its points, its part,
/// and whether it's hovered too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FaceOutline {
    pub(crate) points: Range<u32>,
    pub(crate) part: usize,
    pub(crate) hovered: bool,
}

impl Highlights {
    /// What's drawn of these in `mesh`. Outlined edges that meet are one
    /// polyline in the stream (see [`joined`]), so one's rim doesn't draw
    /// over the other's pixels where they meet. The outlined and selected
    /// edges are kept apart, so an edge both isn't joined to itself.
    /// `faces` are the selected faces to outline, each with whether it's
    /// hovered too; ids the mesh hasn't are left out.
    pub(crate) fn build(&self, mesh: &RenderMesh, faces: &[(u32, bool)]) -> Built {
        let mut stream = EdgeStream::with_capacity(0);
        let start = stream.len();
        for (edge, polyline) in joined(mesh, &self.outlined) {
            stream.push(edge, mesh.positions(), &polyline);
        }
        let outlined = start..stream.len();
        stream.separate();
        let start = stream.len();
        for &edge in &self.selected_edges {
            if let Some(polyline) = mesh.polyline(edge as usize) {
                stream.push(edge, mesh.positions(), polyline);
            }
        }
        let selected = start..stream.len();
        // Apart from the selected, if there are any.
        if !self.second_edges.is_empty() {
            stream.separate();
        }
        let start = stream.len();
        for &edge in &self.second_edges {
            if let Some(polyline) = mesh.polyline(edge as usize) {
                stream.push(edge, mesh.positions(), polyline);
            }
        }
        let second = start..stream.len();
        let face_outlines = face_outlines(mesh, faces, &mut stream);
        let vertices = self
            .vertices
            .iter()
            .filter(|vertex| vertex.hovered || vertex.selected)
            .filter_map(|vertex| {
                let position = *mesh.corners().get(usize::try_from(vertex.corner).ok()?)?;
                let flags =
                    u32::from(vertex.hovered) * HOVERED + u32::from(vertex.selected) * SELECTED;
                Some(VertexInstance { position, flags })
            })
            .collect();
        Built {
            edges: stream.finish(),
            outlined,
            selected,
            second,
            face_outlines,
            vertices,
        }
    }
}

/// The edges bounding each of `faces` in `mesh`, pushed onto `stream`
/// joined, each face's apart from the rest.
fn face_outlines(
    mesh: &RenderMesh,
    faces: &[(u32, bool)],
    stream: &mut EdgeStream,
) -> Vec<FaceOutline> {
    if faces.is_empty() {
        return Vec::new();
    }
    let mut sorted: Vec<u32> = faces.iter().map(|&(face, _)| face).collect();
    sorted.sort_unstable();
    sorted.dedup();
    let mut edges = vec![Vec::new(); sorted.len()];
    for (edge, sides) in mesh.edge_faces().iter().enumerate() {
        let Ok(edge) = u32::try_from(edge) else {
            break;
        };
        for (i, &side) in sides.iter().enumerate() {
            if i == 1 && side == sides[0] {
                continue;
            }
            if let Ok(at) = sorted.binary_search(&side) {
                edges[at].push(edge);
            }
        }
    }
    let part_ends: Vec<usize> = mesh.parts().map(|part| part.faces.end).collect();
    let mut outlines = Vec::new();
    for &(face, hovered) in faces {
        let Ok(at) = sorted.binary_search(&face) else {
            continue;
        };
        let part = part_ends.partition_point(|&end| end <= face as usize);
        if edges[at].is_empty() || part >= part_ends.len() {
            continue;
        }
        stream.separate();
        let start = stream.len();
        for (edge, polyline) in joined(mesh, &edges[at]) {
            stream.push(edge, mesh.positions(), &polyline);
        }
        let points = start..stream.len();
        outlines.push(FaceOutline {
            points,
            part,
            hovered,
        });
    }
    outlines
}

/// `edges` of `mesh`, each once, joined end to end into polylines of its
/// vertices, each named by its lowest edge, until none goes on or the
/// polyline closes. Where more than two meet at a corner, the others start
/// polylines of their own. Ids the mesh hasn't are left out.
fn joined(mesh: &RenderMesh, edges: &[u32]) -> Vec<(u32, Vec<u32>)> {
    let mut edges = edges.to_vec();
    edges.sort_unstable();
    edges.dedup();
    let found: Vec<(u32, &[u32], [u32; 2])> = edges
        .into_iter()
        .filter_map(|edge| {
            let at = edge as usize;
            Some((edge, mesh.polyline(at)?, *mesh.edge_corners().get(at)?))
        })
        .collect();
    // Each edge by each of its corners, sorted by corner.
    let mut ends: Vec<(u32, usize)> = found
        .iter()
        .enumerate()
        .flat_map(|(i, &(_, _, corners))| corners.map(|corner| (corner, i)))
        .collect();
    ends.sort_unstable();
    let mut taken = vec![false; found.len()];
    // The next edge not taken that ends at `corner`, its vertices from
    // there, and the corner it ends at at the other end.
    let next = |corner: u32, taken: &mut [bool]| {
        let from = ends.partition_point(|&(at, _)| at < corner);
        let &(_, i) = ends[from..]
            .iter()
            .take_while(|&&(at, _)| at == corner)
            .find(|&&(_, i)| !taken[i])?;
        taken[i] = true;
        let (_, polyline, [start, end]) = found[i];
        let mut vertices = polyline.to_vec();
        if start != corner {
            vertices.reverse();
            return Some((vertices, start));
        }
        Some((vertices, end))
    };
    let mut joined = Vec::new();
    for i in 0..found.len() {
        if taken[i] {
            continue;
        }
        taken[i] = true;
        let (edge, polyline, [head, mut tail]) = found[i];
        let mut vertices = polyline.to_vec();
        while tail != head
            && let Some((more, end)) = next(tail, &mut taken)
        {
            // The corner's vertex once.
            vertices.extend_from_slice(&more[1..]);
            tail = end;
        }
        let mut front = head;
        while tail != front
            && let Some((more, end)) = next(front, &mut taken)
        {
            vertices.splice(..1, more.into_iter().rev());
            front = end;
        }
        joined.push((edge, vertices));
    }
    joined
}

#[cfg(test)]
mod tests;

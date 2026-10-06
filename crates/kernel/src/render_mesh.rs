use std::ops::Range;
use std::sync::OnceLock;

use glam::Vec3;

use crate::{Aabb, MAX_COORD, MeshUpload};

/// A solid tessellated for drawing: an indexed triangle mesh with
/// per-vertex normals, its triangles grouped by face, and the feature
/// edges to outline as polylines between the faces either side, ending at
/// corners; and the rest of the patches' edges, the wires, as polylines
/// too, for a wireframe.
///
/// Vertices are duplicated where normals split (along sharp edges), and
/// shared where the surface is smooth.
///
/// Meshes are joined by [`RenderMesh::append`], one part per mesh
/// appended, its faces, edges and corners in a run after the last part's.
///
/// A mesh is always drawable: there is a normal for every position, the
/// indices make whole triangles, each face is one or more of them, each
/// edge and wire two or more vertices, the parts take up every face, edge,
/// corner and wire in order, indices and edge and wire vertices refer to vertices that exist
/// and edges to faces and corners of their own part, every corner is where
/// the edges it ends end, every vector is within
/// [`RenderMesh::MAX_VERTICES`] and the others, every normal is finite and
/// every position within [`RenderMesh::MAX_POSITION`], so the renderer's
/// bounds and depth range stay finite.
/// The fields are private so that holds; a mesh built outside the kernel
/// comes in through [`RenderMesh::from_parts`], which checks it.
///
/// It also holds what the renderer uploads beyond its vectors, once it's
/// built ([`RenderMesh::upload`]), so whoever builds the mesh off the UI
/// thread can build that too.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderMesh {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    indices: Vec<u32>,
    face_ends: Vec<u32>,
    edge_vertices: Vec<u32>,
    edge_ends: Vec<u32>,
    edge_faces: Vec<[u32; 2]>,
    corners: Vec<[f32; 3]>,
    edge_corners: Vec<[u32; 2]>,
    wire_vertices: Vec<u32>,
    wire_ends: Vec<u32>,
    part_ends: Vec<[u32; 4]>,
    upload: Upload,
}

/// A mesh's [`MeshUpload`], once built: derived from the rest, so left
/// out of comparisons.
#[derive(Debug, Clone, Default)]
struct Upload(OnceLock<MeshUpload>);

impl PartialEq for Upload {
    fn eq(&self, _: &Upload) -> bool {
        true
    }
}

/// What a [`RenderMesh`] is made of, as [`RenderMesh::from_parts`] takes
/// it and [`RenderMesh::into_parts`] gives it back. See the accessors of
/// the same names for what each holds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshParts {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub face_ends: Vec<u32>,
    pub edge_vertices: Vec<u32>,
    pub edge_ends: Vec<u32>,
    pub edge_faces: Vec<[u32; 2]>,
    pub corners: Vec<[f32; 3]>,
    pub edge_corners: Vec<[u32; 2]>,
    pub wire_vertices: Vec<u32>,
    pub wire_ends: Vec<u32>,
    pub part_ends: Vec<[u32; 4]>,
}

/// One appended mesh's share of a [`RenderMesh`], as ranges of each of its
/// vectors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderPart {
    /// Its faces' ids.
    pub faces: Range<usize>,
    /// Its faces' triangle indices, in [`RenderMesh::indices`].
    pub indices: Range<usize>,
    /// Its edges' ids.
    pub edges: Range<usize>,
    /// Its edges' vertices, in [`RenderMesh::edge_vertices`].
    pub edge_vertices: Range<usize>,
    /// Its corners' ids.
    pub corners: Range<usize>,
    /// Its wires' ids.
    pub wires: Range<usize>,
    /// Its wires' vertices, in [`RenderMesh::wire_vertices`].
    pub wire_vertices: Range<usize>,
}

impl RenderMesh {
    /// The most vertices a mesh may have, about 16 million. Well within
    /// `u32` indices, and the renderer asserts that every part within
    /// these limits fits its GPU buffers.
    pub const MAX_VERTICES: usize = 1 << 24;
    /// The most triangle indices a mesh may have: three for each of about
    /// 16 million triangles.
    pub const MAX_INDICES: usize = 3 << 24;
    /// The most faces a mesh may have: each has a triangle or more.
    pub const MAX_FACES: usize = Self::MAX_INDICES / 3;
    /// The most vertices the edges and the wires may have together, about
    /// 8 million, so that the renderer's stream of their points fits one
    /// GPU buffer.
    pub const MAX_EDGE_POINTS: usize = 1 << 23;
    /// The most edges, or wires, a mesh may have: each has two vertices or
    /// more.
    pub const MAX_EDGE_POLYLINES: usize = Self::MAX_EDGE_POINTS / 2;
    /// The most corners a mesh may have: each ends an edge, and an edge
    /// has two ends.
    pub const MAX_CORNERS: usize = 2 * Self::MAX_EDGE_POLYLINES;
    /// The most parts a mesh may have, about a million: one per mesh
    /// appended, which may be empty.
    pub const MAX_PARTS: usize = 1 << 20;
    /// The largest coordinate a position may have: the farthest a point
    /// within [`MAX_COORD`] gets moved by a length within it, as a sketch
    /// at the limit extruded by the longest distance would be.
    pub const MAX_POSITION: f32 = 2.0 * MAX_COORD;

    /// A mesh of these parts, if they make one; see [`RenderMesh`] for what is
    /// checked.
    pub fn from_parts(parts: MeshParts) -> Result<RenderMesh, MeshError> {
        let MeshParts {
            positions,
            normals,
            indices,
            face_ends,
            edge_vertices,
            edge_ends,
            edge_faces,
            corners,
            edge_corners,
            wire_vertices,
            wire_ends,
            part_ends,
        } = parts;
        if positions.len() > Self::MAX_VERTICES
            || indices.len() > Self::MAX_INDICES
            || face_ends.len() > Self::MAX_FACES
            || (edge_vertices.len()).saturating_add(wire_vertices.len()) > Self::MAX_EDGE_POINTS
            || edge_ends.len() > Self::MAX_EDGE_POLYLINES
            || wire_ends.len() > Self::MAX_EDGE_POLYLINES
            || edge_faces.len() > Self::MAX_EDGE_POLYLINES
            || corners.len() > Self::MAX_CORNERS
            || edge_corners.len() > Self::MAX_EDGE_POLYLINES
            || part_ends.len() > Self::MAX_PARTS
        {
            return Err(MeshError::TooLarge);
        }
        if normals.len() != positions.len() {
            return Err(MeshError::Normals {
                positions: positions.len(),
                normals: normals.len(),
            });
        }
        if !indices.len().is_multiple_of(3) {
            return Err(MeshError::Triangles(indices.len()));
        }
        if !splits(&face_ends, indices.len(), |len| {
            len > 0 && len.is_multiple_of(3)
        }) {
            return Err(MeshError::Ends(MeshPart::FaceEnds));
        }
        if !splits(&edge_ends, edge_vertices.len(), |len| len >= 2) {
            return Err(MeshError::Ends(MeshPart::EdgeEnds));
        }
        if !splits(&wire_ends, wire_vertices.len(), |len| len >= 2) {
            return Err(MeshError::Ends(MeshPart::WireEnds));
        }
        for (part, len) in [
            (MeshPart::EdgeFaces, edge_faces.len()),
            (MeshPart::EdgeCorners, edge_corners.len()),
        ] {
            if len != edge_ends.len() {
                return Err(MeshError::Count {
                    part,
                    len,
                    edges: edge_ends.len(),
                });
            }
        }
        part_runs(
            &part_ends,
            [
                face_ends.len(),
                edge_ends.len(),
                corners.len(),
                wire_ends.len(),
            ],
        )?;
        in_range(MeshPart::Indices, &indices, positions.len())?;
        in_range(MeshPart::EdgeVertices, &edge_vertices, positions.len())?;
        in_range(MeshPart::WireVertices, &wire_vertices, positions.len())?;
        in_parts(&part_ends, &edge_faces, &edge_corners)?;
        if !within(&positions, Self::MAX_POSITION) {
            return Err(MeshError::Values(MeshPart::Positions));
        }
        if !within(&normals, f32::MAX) {
            return Err(MeshError::Values(MeshPart::Normals));
        }
        let mesh = RenderMesh {
            positions,
            normals,
            indices,
            face_ends,
            edge_vertices,
            edge_ends,
            edge_faces,
            corners,
            edge_corners,
            wire_vertices,
            wire_ends,
            part_ends,
            upload: Upload::default(),
        };
        mesh.check_corners()?;
        Ok(mesh)
    }

    /// The parts this mesh is made of, as [`RenderMesh::from_parts`] takes
    /// them.
    pub fn into_parts(self) -> MeshParts {
        MeshParts {
            positions: self.positions,
            normals: self.normals,
            indices: self.indices,
            face_ends: self.face_ends,
            edge_vertices: self.edge_vertices,
            edge_ends: self.edge_ends,
            edge_faces: self.edge_faces,
            corners: self.corners,
            edge_corners: self.edge_corners,
            wire_vertices: self.wire_vertices,
            wire_ends: self.wire_ends,
            part_ends: self.part_ends,
        }
    }

    /// Checks that each edge's corners are where it starts and ends, to
    /// the bit, and that every corner ends an edge. The rest must hold
    /// already.
    fn check_corners(&self) -> Result<(), MeshError> {
        let mut ends_one = vec![false; self.corners.len()];
        for (polyline, corners) in self.polylines().zip(&self.edge_corners) {
            // `==` would take -0 for 0.
            let bits = |p: [f32; 3]| p.map(f32::to_bits);
            let at = |vertex: Option<&u32>| vertex.map(|&v| bits(self.positions[v as usize]));
            for (vertex, &corner) in [polyline.first(), polyline.last()].into_iter().zip(corners) {
                if at(vertex) != Some(bits(self.corners[corner as usize])) {
                    return Err(MeshError::Corners);
                }
                ends_one[corner as usize] = true;
            }
        }
        if ends_one.contains(&false) {
            return Err(MeshError::Corners);
        }
        Ok(())
    }

    pub fn positions(&self) -> &[[f32; 3]] {
        &self.positions
    }

    /// One per position.
    pub fn normals(&self) -> &[[f32; 3]] {
        &self.normals
    }

    /// Three per triangle, face after face.
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// One past each face's last index in [`indices`](Self::indices), in
    /// increasing order, the last one the number of indices. A face is
    /// what users see as one: all the patches of a
    /// [`FaceKey`](crate::mesh::FaceKey) in one solid.
    pub fn face_ends(&self) -> &[u32] {
        &self.face_ends
    }

    /// The feature edges' vertex indices, polyline after polyline. An
    /// edge is the boundary between two faces from one corner to the
    /// next; one that closes on itself repeats its first vertex at its
    /// end.
    pub fn edge_vertices(&self) -> &[u32] {
        &self.edge_vertices
    }

    /// One past each edge's last vertex in
    /// [`edge_vertices`](Self::edge_vertices), in increasing order, the
    /// last one the number of edge vertices.
    pub fn edge_ends(&self) -> &[u32] {
        &self.edge_ends
    }

    /// Each edge's faces: the one whose vertices it runs along (seen from
    /// outside, on its left), then the one on its other side. The two are
    /// the same face where a face creases.
    pub fn edge_faces(&self) -> &[[u32; 2]] {
        &self.edge_faces
    }

    /// Where the edges end: the model's vertices. Each is at an end of one
    /// edge or more, to the bit.
    pub fn corners(&self) -> &[[f32; 3]] {
        &self.corners
    }

    /// Each edge's corners, at its first vertex and at its last. An edge
    /// that closes on itself has the same corner at both.
    pub fn edge_corners(&self) -> &[[u32; 2]] {
        &self.edge_corners
    }

    /// The wires' vertex indices, polyline after polyline: the patches'
    /// edges that aren't feature edges, each from one patch corner to the
    /// next, drawn only in a wireframe.
    pub fn wire_vertices(&self) -> &[u32] {
        &self.wire_vertices
    }

    /// One past each wire's last vertex in
    /// [`wire_vertices`](Self::wire_vertices), in increasing order, the
    /// last one the number of wire vertices.
    pub fn wire_ends(&self) -> &[u32] {
        &self.wire_ends
    }

    /// One past each part's last face, edge, corner and wire, in that
    /// order.
    pub fn part_ends(&self) -> &[[u32; 4]] {
        &self.part_ends
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn face_count(&self) -> usize {
        self.face_ends.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edge_ends.len()
    }

    /// Each face's indices.
    pub fn faces(&self) -> impl ExactSizeIterator<Item = &[u32]> {
        split(&self.indices, &self.face_ends)
    }

    /// Each edge's vertex indices.
    pub fn polylines(&self) -> impl ExactSizeIterator<Item = &[u32]> {
        split(&self.edge_vertices, &self.edge_ends)
    }

    /// Each wire's vertex indices.
    pub fn wires(&self) -> impl ExactSizeIterator<Item = &[u32]> {
        split(&self.wire_vertices, &self.wire_ends)
    }

    /// Where face `face`'s indices are in [`indices`](Self::indices), if
    /// the mesh has it.
    pub fn face_indices(&self, face: usize) -> Option<Range<usize>> {
        run(&self.face_ends, face)
    }

    /// Where edge `edge`'s vertex indices are in
    /// [`edge_vertices`](Self::edge_vertices), if the mesh has it.
    pub fn polyline_range(&self, edge: usize) -> Option<Range<usize>> {
        run(&self.edge_ends, edge)
    }

    /// Edge `edge`'s vertex indices, if the mesh has it.
    pub fn polyline(&self, edge: usize) -> Option<&[u32]> {
        Some(&self.edge_vertices[self.polyline_range(edge)?])
    }

    /// Each part's ranges, in the order they were appended.
    pub fn parts(&self) -> impl ExactSizeIterator<Item = RenderPart> {
        let mut from = [0usize; 4];
        self.part_ends.iter().map(move |to| {
            let to = to.map(|n| n as usize);
            let (faces, edges, wires) = (&self.face_ends, &self.edge_ends, &self.wire_ends);
            let part = RenderPart {
                faces: from[0]..to[0],
                indices: run_start(faces, from[0])..run_start(faces, to[0]),
                edges: from[1]..to[1],
                edge_vertices: run_start(edges, from[1])..run_start(edges, to[1]),
                corners: from[2]..to[2],
                wires: from[3]..to[3],
                wire_vertices: run_start(wires, from[3])..run_start(wires, to[3]),
            };
            from = to;
            part
        })
    }

    /// Appends another mesh, as [`append_at`](Self::append_at) does
    /// without moving it.
    pub fn append(&mut self, other: &RenderMesh) -> Result<(), MeshError> {
        self.append_at(other, Vec3::ZERO)
    }

    /// Appends another mesh's parts after ours, its vertices and corners
    /// moved by `offset`.
    ///
    /// Fails, leaving `self` as it was, with [`MeshError::TooLarge`] if
    /// the result would have more vertices, indices, edges or any other
    /// part than [`RenderMesh::MAX_VERTICES`] and the others allow, and
    /// with [`MeshError::Values`] if a moved position would be past
    /// [`RenderMesh::MAX_POSITION`]. Documents are tessellated by
    /// appending one mesh per body, and a file can hold any number of
    /// bodies.
    pub fn append_at(&mut self, other: &RenderMesh, offset: Vec3) -> Result<(), MeshError> {
        let sizes = [
            (
                self.positions.len(),
                other.positions.len(),
                Self::MAX_VERTICES,
            ),
            (self.indices.len(), other.indices.len(), Self::MAX_INDICES),
            (self.face_ends.len(), other.face_ends.len(), Self::MAX_FACES),
            (
                (self.edge_vertices.len()).saturating_add(self.wire_vertices.len()),
                (other.edge_vertices.len()).saturating_add(other.wire_vertices.len()),
                Self::MAX_EDGE_POINTS,
            ),
            (
                self.wire_ends.len(),
                other.wire_ends.len(),
                Self::MAX_EDGE_POLYLINES,
            ),
            (
                self.edge_ends.len(),
                other.edge_ends.len(),
                Self::MAX_EDGE_POLYLINES,
            ),
            (self.corners.len(), other.corners.len(), Self::MAX_CORNERS),
            (self.part_ends.len(), other.part_ends.len(), Self::MAX_PARTS),
        ];
        if !sizes
            .iter()
            .all(|&(ours, theirs, max)| ours.checked_add(theirs).is_some_and(|n| n <= max))
        {
            return Err(MeshError::TooLarge);
        }
        // Moving is monotonic, so the moved bounds' corners are the
        // extremes of every moved position, and every corner is at a
        // position.
        if let Some(Aabb { min, max }) = other.bounds()
            && !within(
                &[(min + offset).to_array(), (max + offset).to_array()],
                Self::MAX_POSITION,
            )
        {
            return Err(MeshError::Values(MeshPart::Positions));
        }
        // Every index and id of `other` is below its own count, so adding
        // our count stays within the limits, checked above.
        let base = |len: usize| u32::try_from(len).map_err(|_| MeshError::TooLarge);
        let vertices = base(self.positions.len())?;
        let indices = base(self.indices.len())?;
        let faces = base(self.face_ends.len())?;
        let edge_points = base(self.edge_vertices.len())?;
        let edges = base(self.edge_ends.len())?;
        let corners = base(self.corners.len())?;
        let wire_points = base(self.wire_vertices.len())?;
        let wires = base(self.wire_ends.len())?;

        self.upload = Upload::default();
        // Corners move as their positions do, so they stay equal.
        let moved = |p: &[f32; 3]| (Vec3::from(*p) + offset).to_array();
        self.positions.extend(other.positions.iter().map(moved));
        // Moving doesn't turn faces.
        self.normals.extend_from_slice(&other.normals);
        self.indices
            .extend(other.indices.iter().map(|i| vertices + i));
        self.face_ends
            .extend(other.face_ends.iter().map(|end| indices + end));
        self.edge_vertices
            .extend(other.edge_vertices.iter().map(|v| vertices + v));
        self.edge_ends
            .extend(other.edge_ends.iter().map(|end| edge_points + end));
        self.edge_faces
            .extend(other.edge_faces.iter().map(|f| f.map(|f| faces + f)));
        self.corners.extend(other.corners.iter().map(moved));
        self.edge_corners
            .extend(other.edge_corners.iter().map(|c| c.map(|c| corners + c)));
        self.wire_vertices
            .extend(other.wire_vertices.iter().map(|v| vertices + v));
        self.wire_ends
            .extend(other.wire_ends.iter().map(|end| wire_points + end));
        self.part_ends.extend(
            other
                .part_ends
                .iter()
                .map(|&[f, e, c, w]| [faces + f, edges + e, corners + c, wires + w]),
        );
        Ok(())
    }

    /// Every edge's segments, as pairs of vertex indices, edge after edge.
    pub fn edge_segments(&self) -> impl Iterator<Item = [u32; 2]> + '_ {
        self.polylines()
            .flat_map(|polyline| polyline.windows(2).map(|pair| [pair[0], pair[1]]))
    }

    /// What the renderer uploads of the mesh beyond its vectors, built the
    /// first time it's asked for: the regeneration lane asks, so the UI
    /// thread doesn't build it.
    pub fn upload(&self) -> &MeshUpload {
        self.upload.0.get_or_init(|| MeshUpload::new(self))
    }

    /// Gives the mesh `upload`, which [`MeshUpload::from_parts`] checked
    /// against it, unless it has one already.
    pub fn set_upload(&self, upload: MeshUpload) {
        let _ = self.upload.0.set(upload);
    }

    /// Axis-aligned bounds, or `None` for an empty mesh.
    pub fn bounds(&self) -> Option<Aabb> {
        Aabb::around(&self.positions)
    }
}

/// Whether `ends`, each one past a run's last item, split `total` items
/// into runs, each of a length `whole` takes.
pub(crate) fn splits(ends: &[u32], total: usize, whole: impl Fn(usize) -> bool) -> bool {
    let mut start = 0usize;
    for &end in ends {
        let Ok(end) = usize::try_from(end) else {
            return false;
        };
        if end.checked_sub(start).is_none_or(|len| !whole(len)) {
            return false;
        }
        start = end;
    }
    start == total
}

/// Where run `i` of those `ends` split items into is, if there is one.
fn run(ends: &[u32], i: usize) -> Option<Range<usize>> {
    let end = *ends.get(i)? as usize;
    Some(run_start(ends, i)..end)
}

/// Where run `i` of those `ends` split items into starts, where the runs
/// before it end, for `i` up to `ends.len()`.
fn run_start(ends: &[u32], i: usize) -> usize {
    i.checked_sub(1).map_or(0, |before| ends[before] as usize)
}

/// The runs `ends` split `items` into, which they must ([`splits`]).
pub(crate) fn split<'a, T>(
    items: &'a [T],
    ends: &'a [u32],
) -> impl ExactSizeIterator<Item = &'a [T]> + 'a {
    let mut start = 0;
    ends.iter().map(move |&end| {
        let run = &items[start..end as usize];
        start = end as usize;
        run
    })
}

/// Checks that the parts' ends of faces, edges, corners and wires each
/// never fall, and that the last are `totals` (none if there are no
/// parts).
fn part_runs(part_ends: &[[u32; 4]], totals: [usize; 4]) -> Result<(), MeshError> {
    let mut start = [0usize; 4];
    for ends in part_ends {
        for (start, &end) in start.iter_mut().zip(ends) {
            let end = usize::try_from(end).map_err(|_| MeshError::Ends(MeshPart::PartEnds))?;
            if end < *start {
                return Err(MeshError::Ends(MeshPart::PartEnds));
            }
            *start = end;
        }
    }
    if start != totals {
        return Err(MeshError::Ends(MeshPart::PartEnds));
    }
    Ok(())
}

/// Checks that each edge's faces and corners are of its own part. The
/// parts must take up every face, edge and corner.
fn in_parts(
    part_ends: &[[u32; 4]],
    edge_faces: &[[u32; 2]],
    edge_corners: &[[u32; 2]],
) -> Result<(), MeshError> {
    let mut from = [0u32; 4];
    for &to in part_ends {
        for edge in from[1] as usize..to[1] as usize {
            for (part, ids, range) in [
                (MeshPart::EdgeFaces, edge_faces[edge], from[0]..to[0]),
                (MeshPart::EdgeCorners, edge_corners[edge], from[2]..to[2]),
            ] {
                if let Some(&id) = ids.iter().find(|id| !range.contains(id)) {
                    return Err(MeshError::OutsidePart { part, id });
                }
            }
        }
        from = to;
    }
    Ok(())
}

/// Checks that every vertex index in `part` is below `vertices`.
fn in_range(part: MeshPart, indices: &[u32], vertices: usize) -> Result<(), MeshError> {
    match indices
        .iter()
        .find(|&&index| !usize::try_from(index).is_ok_and(|index| index < vertices))
    {
        Some(&index) => Err(MeshError::OutOfRange {
            part,
            index,
            vertices,
        }),
        None => Ok(()),
    }
}

/// Whether every coordinate in `values` is within `max` of zero, and so
/// finite.
pub(crate) fn within(values: &[[f32; 3]], max: f32) -> bool {
    values.as_flattened().iter().all(|v| v.abs() <= max)
}

/// One of the parts a [`RenderMesh`] is made of, as
/// [`RenderMesh::from_parts`] takes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshPart {
    Positions,
    Normals,
    Indices,
    FaceEnds,
    EdgeVertices,
    EdgeEnds,
    EdgeFaces,
    Corners,
    EdgeCorners,
    WireVertices,
    WireEnds,
    PartEnds,
}

impl std::fmt::Display for MeshPart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            MeshPart::Positions => "positions",
            MeshPart::Normals => "normals",
            MeshPart::Indices => "indices",
            MeshPart::FaceEnds => "face ends",
            MeshPart::EdgeVertices => "edge vertices",
            MeshPart::EdgeEnds => "edge ends",
            MeshPart::EdgeFaces => "edge faces",
            MeshPart::Corners => "corners",
            MeshPart::EdgeCorners => "edge corners",
            MeshPart::WireVertices => "wire vertices",
            MeshPart::WireEnds => "wire ends",
            MeshPart::PartEnds => "part ends",
        })
    }
}

/// Why parts don't make a [`RenderMesh`], or a mesh can't be appended to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshError {
    /// The mesh would have more vertices, indices, edges or any other
    /// part than [`RenderMesh::MAX_VERTICES`] and the others allow.
    TooLarge,
    /// There aren't as many normals as positions.
    Normals { positions: usize, normals: usize },
    /// This many indices don't make whole triangles.
    Triangles(usize),
    /// These ends ([`MeshPart::FaceEnds`], [`MeshPart::EdgeEnds`],
    /// [`MeshPart::WireEnds`] or [`MeshPart::PartEnds`]) fall, don't end at the last of what they
    /// split, or leave a face without a whole triangle or an edge without
    /// two vertices.
    Ends(MeshPart),
    /// There aren't as many of an edge's part
    /// ([`MeshPart::EdgeFaces`] or [`MeshPart::EdgeCorners`]) as edges.
    Count {
        part: MeshPart,
        len: usize,
        edges: usize,
    },
    /// An index, edge vertex or wire vertex refers to a vertex past the last.
    OutOfRange {
        part: MeshPart,
        index: u32,
        vertices: usize,
    },
    /// An edge refers to a face or corner ([`MeshPart::EdgeFaces`] or
    /// [`MeshPart::EdgeCorners`]) that isn't of its part.
    OutsidePart { part: MeshPart, id: u32 },
    /// A corner isn't where an edge it ends starts or ends, or ends no
    /// edge.
    Corners,
    /// A position isn't within [`RenderMesh::MAX_POSITION`]
    /// ([`MeshPart::Positions`]), or a normal isn't finite
    /// ([`MeshPart::Normals`]).
    Values(MeshPart),
}

impl std::fmt::Display for MeshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MeshError::TooLarge => {
                f.write_str("the mesh has more vertices, indices or edges than the kernel allows")
            }
            MeshError::Normals { positions, normals } => {
                write!(f, "mesh has {normals} normals for {positions} positions")
            }
            MeshError::Triangles(n) => write!(f, "{n} mesh indices don't make whole triangles"),
            MeshError::Ends(part) => write!(f, "mesh {part} don't split what they end"),
            MeshError::Count { part, len, edges } => {
                write!(f, "mesh has {len} {part} for {edges} edges")
            }
            MeshError::OutOfRange {
                part,
                index,
                vertices,
            } => write!(
                f,
                "mesh {part} refer to vertex {index} of {vertices} vertices"
            ),
            MeshError::OutsidePart { part, id } => {
                write!(f, "mesh {part} refer to {id}, outside their part")
            }
            MeshError::Corners => f.write_str("mesh corners aren't where their edges end"),
            MeshError::Values(part) => write!(f, "mesh {part} hold values out of range"),
        }
    }
}

impl std::error::Error for MeshError {}

#[cfg(test)]
mod tests;

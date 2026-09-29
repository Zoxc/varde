use glam::Vec3;

use crate::{Aabb, MAX_COORD};

/// A solid tessellated for drawing: an indexed triangle mesh with
/// per-vertex normals and the feature edges to outline.
///
/// Vertices are duplicated where normals split (along sharp edges), and
/// shared where the surface is smooth.
///
/// A mesh is always drawable: there is a normal for every position, the
/// indices make whole triangles, indices and edges refer to vertices that
/// exist, every part is within [`RenderMesh::MAX_VERTICES`] and the others,
/// every normal is finite and every position within
/// [`RenderMesh::MAX_POSITION`], so the renderer's bounds and depth range
/// stay finite.
/// The fields are private so that holds; a mesh built outside the kernel
/// comes in through [`RenderMesh::from_parts`], which checks it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderMesh {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    indices: Vec<u32>,
    edges: Vec<[u32; 2]>,
}

impl RenderMesh {
    /// The most vertices a mesh may have, about 16 million. Well within
    /// `u32` indices, and the renderer asserts that every part within
    /// these limits fits its GPU buffers.
    pub const MAX_VERTICES: usize = 1 << 24;
    /// The most triangle indices a mesh may have: three for each of about
    /// 16 million triangles.
    pub const MAX_INDICES: usize = 3 << 24;
    /// The most edges a mesh may have, about 33 million.
    pub const MAX_EDGES: usize = 1 << 25;
    /// The largest coordinate a position may have: the farthest a solid
    /// whose [`bounds`](crate::Solid::bounds) are within [`MAX_COORD`] of
    /// its origin reaches from a position within it.
    pub const MAX_POSITION: f32 = 2.0 * MAX_COORD;

    /// A mesh of these parts, if they make one; see [`RenderMesh`] for what is
    /// checked.
    pub fn from_parts(
        positions: Vec<[f32; 3]>,
        normals: Vec<[f32; 3]>,
        indices: Vec<u32>,
        edges: Vec<[u32; 2]>,
    ) -> Result<RenderMesh, MeshError> {
        if positions.len() > Self::MAX_VERTICES
            || indices.len() > Self::MAX_INDICES
            || edges.len() > Self::MAX_EDGES
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
        in_range(MeshPart::Indices, &indices, positions.len())?;
        in_range(MeshPart::Edges, edges.as_flattened(), positions.len())?;
        if !within(&positions, Self::MAX_POSITION) {
            return Err(MeshError::Values(MeshPart::Positions));
        }
        if !within(&normals, f32::MAX) {
            return Err(MeshError::Values(MeshPart::Normals));
        }
        Ok(RenderMesh {
            positions,
            normals,
            indices,
            edges,
        })
    }

    pub fn positions(&self) -> &[[f32; 3]] {
        &self.positions
    }

    /// One per position.
    pub fn normals(&self) -> &[[f32; 3]] {
        &self.normals
    }

    /// Three per triangle.
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// Feature edges (face boundaries) as pairs of vertex indices.
    pub fn edges(&self) -> &[[u32; 2]] {
        &self.edges
    }

    /// Appends another mesh, its vertices moved by `offset`.
    ///
    /// Fails, leaving `self` as it was, with [`MeshError::TooLarge`] if
    /// the result would have more vertices, indices or edges than
    /// [`RenderMesh::MAX_VERTICES`] and the others allow, and with
    /// [`MeshError::Values`] if a moved position would be past
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
            (self.edges.len(), other.edges.len(), Self::MAX_EDGES),
        ];
        if !sizes
            .iter()
            .all(|&(ours, theirs, max)| ours.checked_add(theirs).is_some_and(|n| n <= max))
        {
            return Err(MeshError::TooLarge);
        }
        // Moving is monotonic, so the moved bounds' corners are the
        // extremes of every moved position.
        if let Some(Aabb { min, max }) = other.bounds()
            && !within(
                &[(min + offset).to_array(), (max + offset).to_array()],
                Self::MAX_POSITION,
            )
        {
            return Err(MeshError::Values(MeshPart::Positions));
        }
        // Every index of `other` is below its vertex count, so `base + i`
        // is below `MAX_VERTICES`, checked above.
        let base = u32::try_from(self.positions.len()).map_err(|_| MeshError::TooLarge)?;

        self.positions.extend(
            other
                .positions
                .iter()
                .map(|p| (Vec3::from(*p) + offset).to_array()),
        );
        // Moving doesn't turn faces.
        self.normals.extend_from_slice(&other.normals);
        self.indices.extend(other.indices.iter().map(|i| base + i));
        self.edges
            .extend(other.edges.iter().map(|[a, b]| [base + a, base + b]));
        Ok(())
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Axis-aligned bounds, or `None` for an empty mesh.
    pub fn bounds(&self) -> Option<Aabb> {
        Aabb::around(&self.positions)
    }
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
    Edges,
}

impl std::fmt::Display for MeshPart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            MeshPart::Positions => "positions",
            MeshPart::Normals => "normals",
            MeshPart::Indices => "indices",
            MeshPart::Edges => "edges",
        })
    }
}

/// Why parts don't make a [`RenderMesh`], or a mesh can't be appended to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshError {
    /// The mesh would have more vertices, indices or edges than
    /// [`RenderMesh::MAX_VERTICES`] and the others allow.
    TooLarge,
    /// There aren't as many normals as positions.
    Normals { positions: usize, normals: usize },
    /// This many indices don't make whole triangles.
    Triangles(usize),
    /// An index or edge refers to a vertex past the last.
    OutOfRange {
        part: MeshPart,
        index: u32,
        vertices: usize,
    },
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
            MeshError::OutOfRange {
                part,
                index,
                vertices,
            } => write!(
                f,
                "mesh {part} refer to vertex {index} of {vertices} vertices"
            ),
            MeshError::Values(part) => write!(f, "mesh {part} hold values out of range"),
        }
    }
}

impl std::error::Error for MeshError {}

#[cfg(test)]
mod tests;

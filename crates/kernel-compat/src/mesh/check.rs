use crate::MAX_PATCHES;
use crate::patch::PatchError;

/// Which invariant a mesh breaks, and where: the first failure found, in
/// the order of the invariants, then of halfedge, vertex, edge or triangle
/// index (pairs of triangles in lexicographic order), so the same mesh
/// always gives the same error.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CheckError {
    /// More than [`MAX_PATCHES`] triangles.
    TooManyPatches(usize),
    /// More vertices than halfedges, a number of edges other than half the
    /// halfedges, or more than [`MAX_PATCHES`] faces or aliases.
    Counts,
    /// Halfedge `h` names a vertex, pair or edge that doesn't exist, or its
    /// triangle a face that doesn't.
    Index(u32),
    /// Halfedge `h`'s pair isn't paired back with it, or doesn't run the
    /// other way between the same vertices.
    Pair(u32),
    /// Halfedge `h` starts and ends at the same vertex.
    Loop(u32),
    /// Alias `i` of [`Mesh::aliases`](super::Mesh::aliases) names a face that doesn't exist or
    /// the face's own key, or isn't after the one before it.
    Alias(u32),
    /// Halfedge `h` runs between the same vertices, the same way, as
    /// another.
    DirectedEdge(u32),
    /// Vertex `v` starts no halfedge, or its triangles form more than one
    /// fan.
    Fan(u32),
    /// Halfedge `h` and its pair name different edges.
    SharedEdge(u32),
    /// Edge `e` isn't used by exactly one pair of halfedges.
    EdgeUse(u32),
    /// Triangle `t` has a coordinate or weight out of bounds.
    Patch(u32, PatchError),
    /// Triangle `t` fails the fold check.
    Fold(u32),
    /// Triangle `t` doesn't lie on its face's surface within the
    /// resolution, or the surface isn't well defined.
    Face(u32),
    /// Triangle `t` is on a face claiming a plane, or with a plane form,
    /// but faces against the plane's normal (or its normal isn't well
    /// defined) at its middle.
    FacesAgainst(u32),
    /// Triangles that share no vertex have hulls within the resolution.
    Hull(u32, u32),
    /// Triangles sharing an edge aren't split by a plane through it (or,
    /// for a curved edge, the cylinder over it or a quadric of the pencil
    /// of that cylinder and the edge's plane).
    EdgeNeighbours(u32, u32),
    /// Triangles sharing only a vertex aren't split by a plane through it.
    VertexNeighbours(u32, u32),
    /// Triangles sharing all three corners.
    SameCorners(u32, u32),
    /// The shell (connected part) whose lowest triangle is `t` faces the
    /// wrong way for where it lies among the others: inside out, or
    /// facing out inside another solid, or in where it isn't inside one.
    /// Also for a shell whose volume is too close to zero to tell.
    InsideOut(u32),
}

impl std::fmt::Display for CheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckError::TooManyPatches(n) => write!(f, "{n} patches are more than {MAX_PATCHES}"),
            CheckError::Counts => write!(
                f,
                "the vertex, edge or face count doesn't fit the triangles"
            ),
            CheckError::Index(h) => write!(f, "halfedge {h} names something that doesn't exist"),
            CheckError::Pair(h) => write!(f, "halfedge {h} isn't paired with one running back"),
            CheckError::Loop(h) => write!(f, "halfedge {h} starts and ends at one vertex"),
            CheckError::Alias(i) => {
                write!(f, "alias {i} names no face or its own, or is out of order")
            }
            CheckError::DirectedEdge(h) => write!(f, "halfedge {h} runs the same way as another"),
            CheckError::Fan(v) => write!(f, "vertex {v} doesn't have exactly one fan"),
            CheckError::SharedEdge(h) => {
                write!(f, "halfedge {h} and its pair name different edges")
            }
            CheckError::EdgeUse(e) => write!(f, "edge {e} isn't used by exactly one pair"),
            CheckError::Patch(t, e) => write!(f, "patch {t}: {e}"),
            CheckError::Fold(t) => write!(f, "patch {t} may fold"),
            CheckError::Face(t) => write!(f, "patch {t} is off its face's surface"),
            CheckError::FacesAgainst(t) => write!(f, "patch {t} faces against its face's plane"),
            CheckError::Hull(a, b) => write!(f, "the hulls of patches {a} and {b} come too close"),
            CheckError::EdgeNeighbours(a, b) => {
                write!(f, "patches {a} and {b} aren't split by their shared edge")
            }
            CheckError::VertexNeighbours(a, b) => {
                write!(f, "patches {a} and {b} aren't split at their shared vertex")
            }
            CheckError::SameCorners(a, b) => write!(f, "patches {a} and {b} share all corners"),
            CheckError::InsideOut(t) => write!(
                f,
                "the shell with patch {t} faces the wrong way for where it lies"
            ),
        }
    }
}

impl std::error::Error for CheckError {}

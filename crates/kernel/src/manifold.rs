//! Indexed triangle meshes that are closed, oriented 2-manifolds, for
//! export ([`Solid::manifold_mesh`](crate::Solid::manifold_mesh)).
//!
//! The triangles come from the tessellation, welded by the patch mesh's
//! own vertices and shared edges (never by distance), and the mesh is
//! checked before anyone gets it: [`ManifoldMesh::new`] is the only way
//! to make one, deserializing included.

use glam::DVec3;
use serde::{Deserialize, Serialize};

use crate::boolean::exact::{Num, Pred, Pt, sign, sub};
use crate::patch::Bounds3;
use crate::{MAX_COORD, RenderMesh};

/// A closed, oriented 2-manifold of triangles, as file formats such as
/// 3MF want it: vertices shared by index, and always, as
/// [`ManifoldMesh::new`] checks,
///
/// - at least one triangle, at most [`ManifoldMesh::MAX_VERTICES`]
///   vertices and [`ManifoldMesh::MAX_TRIANGLES`] triangles, every index
///   in range and every coordinate finite and within
///   [`ManifoldMesh::MAX_POSITION`];
/// - no triangle with a vertex twice, or whose corners lie on a line
///   (exactly, in `f64`), and no two vertices at the same position;
/// - every edge used by exactly two triangles, running it opposite ways;
/// - the triangles round each vertex one fan, and every vertex in some
///   triangle;
/// - a positive signed volume: the triangles face out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Unchecked")]
pub struct ManifoldMesh {
    positions: Vec<[f64; 3]>,
    triangles: Vec<[u32; 3]>,
}

/// A [`ManifoldMesh`] as it is serialized, checked on the way in.
#[derive(Deserialize)]
struct Unchecked {
    positions: Vec<[f64; 3]>,
    triangles: Vec<[u32; 3]>,
}

impl TryFrom<Unchecked> for ManifoldMesh {
    type Error = ManifoldError;

    fn try_from(parts: Unchecked) -> Result<Self, Self::Error> {
        ManifoldMesh::new(parts.positions, parts.triangles)
    }
}

impl ManifoldMesh {
    /// The most vertices a mesh may have: as many as a [`RenderMesh`].
    pub const MAX_VERTICES: usize = RenderMesh::MAX_VERTICES;
    /// The most triangles a mesh may have: as many as a [`RenderMesh`].
    pub const MAX_TRIANGLES: usize = RenderMesh::MAX_INDICES / 3;
    /// The largest coordinate a position may have, as for a
    /// [`RenderMesh`].
    pub const MAX_POSITION: f64 = 2.0 * MAX_COORD as f64;

    /// The mesh of these parts, if it is one: see [`ManifoldMesh`] for
    /// what is checked, in that order, the first failure (by the lowest
    /// triangle, vertex or edge) given.
    pub fn new(
        positions: Vec<[f64; 3]>,
        triangles: Vec<[u32; 3]>,
    ) -> Result<ManifoldMesh, ManifoldError> {
        let mesh = ManifoldMesh {
            positions,
            triangles,
        };
        mesh.check()?;
        Ok(mesh)
    }

    pub fn positions(&self) -> &[[f64; 3]] {
        &self.positions
    }

    /// Each triangle's vertices, counter-clockwise seen from outside.
    pub fn triangles(&self) -> &[[u32; 3]] {
        &self.triangles
    }

    /// The volume it encloses, a sixth of the sum of the triangles'
    /// determinants about the middle of its box, added in triangle order.
    pub fn volume(&self) -> f64 {
        signed_volume(&self.positions, &self.triangles)
    }

    fn point(&self, v: u32) -> DVec3 {
        DVec3::from_array(self.positions[v as usize])
    }

    fn check(&self) -> Result<(), ManifoldError> {
        let (positions, triangles) = (&self.positions, &self.triangles);
        if positions.len() > Self::MAX_VERTICES || triangles.len() > Self::MAX_TRIANGLES {
            return Err(ManifoldError::TooLarge);
        }
        if triangles.is_empty() {
            return Err(ManifoldError::Empty);
        }
        let vertices = positions.len();
        if let Some(v) =
            (positions.iter()).position(|p| !p.iter().all(|x| x.abs() <= Self::MAX_POSITION))
        {
            return Err(ManifoldError::Position(v as u32));
        }
        for (t, tri) in triangles.iter().enumerate() {
            if tri.iter().any(|&v| v as usize >= vertices) {
                return Err(ManifoldError::Index(t as u32));
            }
            if tri[0] == tri[1] || tri[1] == tri[2] || tri[2] == tri[0] {
                return Err(ManifoldError::RepeatedVertex(t as u32));
            }
        }
        if let Some(t) = (triangles.iter()).position(|tri| self.degenerate(tri)) {
            return Err(ManifoldError::Degenerate(t as u32));
        }

        // Vertices at one position. `-0.0 + 0.0` is `+0.0`, so the bits of
        // equal coordinates are equal.
        let mut by_position: Vec<([u64; 3], u32)> = (positions.iter().enumerate())
            .map(|(v, p)| (p.map(|x| (x + 0.0).to_bits()), v as u32))
            .collect();
        by_position.sort_unstable();
        if let Some(pair) = by_position.windows(2).find(|w| w[0].0 == w[1].0) {
            return Err(ManifoldError::Coincident(pair[0].1, pair[1].1));
        }
        drop(by_position);

        // Each directed edge once, and its reverse once.
        let mut directed: Vec<(u32, u32)> = (triangles.iter())
            .flat_map(|&[a, b, c]| [(a, b), (b, c), (c, a)])
            .collect();
        directed.sort_unstable();
        // The undirected edges, each with how many triangles use it
        // either way.
        let mut undirected: Vec<(u32, u32)> = (directed.iter())
            .map(|&(a, b)| (a.min(b), a.max(b)))
            .collect();
        undirected.sort_unstable();
        let mut i = 0;
        while i < undirected.len() {
            let edge = undirected[i];
            let uses = undirected[i..].iter().take_while(|&&e| e == edge).count();
            if uses != 2 {
                return Err(ManifoldError::EdgeUse {
                    a: edge.0,
                    b: edge.1,
                    uses: uses as u32,
                });
            }
            i += uses;
        }
        drop(undirected);
        if let Some(w) = directed.windows(2).find(|w| w[0] == w[1]) {
            return Err(ManifoldError::Orientation {
                a: w[0].0.min(w[0].1),
                b: w[0].0.max(w[0].1),
            });
        }
        drop(directed);

        // Round each vertex `v`, its triangles `(v, b, c)` link `b` to
        // `c`; with every directed edge once, that's a permutation of the
        // vertex's neighbours, and one fan is one cycle of it.
        let mut corners: Vec<(u32, u32, u32)> = (triangles.iter())
            .flat_map(|&[a, b, c]| [(a, b, c), (b, c, a), (c, a, b)])
            .collect();
        corners.sort_unstable();
        let mut next_vertex = 0u32;
        let mut i = 0;
        while i < corners.len() {
            let v = corners[i].0;
            if v != next_vertex {
                return Err(ManifoldError::UnusedVertex(next_vertex));
            }
            let fan = &corners[i..];
            let n = fan.iter().take_while(|c| c.0 == v).count();
            let fan = &fan[..n];
            let start = fan[0].1;
            let mut at = fan[0].2;
            let mut steps = 1;
            while at != start && steps <= n {
                match fan.binary_search_by_key(&at, |c| c.1) {
                    Ok(k) => at = fan[k].2,
                    // Every edge has two triangles, so every neighbour
                    // starts a corner; kept as a failure all the same.
                    Err(_) => return Err(ManifoldError::Fan(v)),
                }
                steps += 1;
            }
            if steps != n {
                return Err(ManifoldError::Fan(v));
            }
            next_vertex = v + 1;
            i += n;
        }
        if (next_vertex as usize) < vertices {
            return Err(ManifoldError::UnusedVertex(next_vertex));
        }

        if self.volume() <= 0.0 {
            return Err(ManifoldError::InsideOut);
        }
        Ok(())
    }

    /// Whether `tri`'s corners lie on a line: every component of
    /// `(b − a) × (c − a)` exactly zero.
    fn degenerate(&self, tri: &[u32; 3]) -> bool {
        let p = tri.map(|v| self.point(v));
        (0..3).all(|axis| sign(&CrossComponent { p, axis }) == 0)
    }
}

/// A sixth of the sum of `triangles`' determinants about the middle of
/// the box round `positions`, in triangle order.
fn signed_volume(positions: &[[f64; 3]], triangles: &[[u32; 3]]) -> f64 {
    let points: Vec<DVec3> = positions.iter().map(|&p| DVec3::from_array(p)).collect();
    let Some(bounds) = Bounds3::around(&points) else {
        return 0.0;
    };
    let o = (bounds.min + bounds.max) * 0.5;
    let sum: f64 = (triangles.iter())
        .map(|tri| {
            let [a, b, c] = tri.map(|v| points[v as usize] - o);
            a.dot(b.cross(c))
        })
        .sum();
    sum / 6.0
}

/// Component `axis` of `(p1 − p0) × (p2 − p0)`, for its exact sign.
struct CrossComponent {
    p: [DVec3; 3],
    axis: usize,
}

impl Pred for CrossComponent {
    fn eval<N: Num>(&self) -> N {
        let [a, b, c] = self.p.map(|p| Pt { p, n: None }.v3::<N>());
        let (u, v) = (sub(&b, &a), sub(&c, &a));
        let (i, j) = ((self.axis + 1) % 3, (self.axis + 2) % 3);
        u[i].mul(&v[j]).sub(&u[j].mul(&v[i]))
    }
}

/// Why a solid gives no [`ManifoldMesh`], or parts don't make one.
/// Vertices and triangles are numbered from 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifoldError {
    /// More than [`ManifoldMesh::MAX_VERTICES`] vertices or
    /// [`ManifoldMesh::MAX_TRIANGLES`] triangles.
    TooLarge,
    /// No triangles: the empty solid.
    Empty,
    /// This vertex isn't finite or is past
    /// [`ManifoldMesh::MAX_POSITION`].
    Position(u32),
    /// This triangle names a vertex past the last.
    Index(u32),
    /// This triangle names a vertex twice.
    RepeatedVertex(u32),
    /// This triangle's corners lie on a line.
    Degenerate(u32),
    /// These two vertices are at the same position.
    Coincident(u32, u32),
    /// The edge between these vertices is used by this many triangles,
    /// not two.
    EdgeUse { a: u32, b: u32, uses: u32 },
    /// Both triangles along the edge between these vertices run it the
    /// same way.
    Orientation { a: u32, b: u32 },
    /// The triangles round this vertex make more than one fan.
    Fan(u32),
    /// No triangle uses this vertex.
    UnusedVertex(u32),
    /// The signed volume isn't positive: the triangles face in.
    InsideOut,
}

impl std::fmt::Display for ManifoldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManifoldError::TooLarge => {
                f.write_str("the mesh would have more vertices or triangles than allowed")
            }
            ManifoldError::Empty => f.write_str("the mesh has no triangles"),
            ManifoldError::Position(v) => write!(f, "vertex {v} is out of range"),
            ManifoldError::Index(t) => write!(f, "triangle {t} names a vertex that doesn't exist"),
            ManifoldError::RepeatedVertex(t) => write!(f, "triangle {t} names a vertex twice"),
            ManifoldError::Degenerate(t) => write!(f, "triangle {t} has no area"),
            ManifoldError::Coincident(a, b) => {
                write!(f, "vertices {a} and {b} are at the same position")
            }
            ManifoldError::EdgeUse { a, b, uses } => write!(
                f,
                "the edge from vertex {a} to {b} is used by {uses} triangles, not two"
            ),
            ManifoldError::Orientation { a, b } => write!(
                f,
                "the triangles along the edge from vertex {a} to {b} face opposite ways"
            ),
            ManifoldError::Fan(v) => write!(f, "the surface pinches at vertex {v}"),
            ManifoldError::UnusedVertex(v) => write!(f, "vertex {v} is in no triangle"),
            ManifoldError::InsideOut => f.write_str("the mesh is inside out"),
        }
    }
}

impl std::error::Error for ManifoldError {}

#[cfg(test)]
mod tests;

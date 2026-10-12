use std::collections::BTreeMap;

use glam::DVec3;

use super::{Edge, Face, FaceKey, Halfedge, LookupMap, Mesh, Tri};
use crate::MAX_PATCHES;
use crate::patch::{Conic3, Patch, PatchError, cylinder_strip};

/// Builds a [`Mesh`] from vertices and triangles given by vertex ids,
/// pairing halfedges by the vertices they run between: `a → b` pairs with
/// `b → a`. That is topology, never a comparison of positions. Edges are
/// straight unless given a curve with [`MeshBuilder::edge`].
///
/// The mesh built has its halfedges paired and its `Edge` records shared;
/// it still has to pass [`Mesh::check`].
#[derive(Debug, Clone, Default)]
pub struct MeshBuilder {
    verts: Vec<DVec3>,
    faces: Vec<Face>,
    tris: Vec<([u32; 3], u32)>,
    curves: BTreeMap<(u32, u32), Edge>,
    aliases: Vec<(u32, FaceKey)>,
}

/// Why [`MeshBuilder::build`] can't pair the triangles up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildError {
    /// More than [`MAX_PATCHES`] triangles.
    TooManyPatches(usize),
    /// A triangle naming a vertex or face that wasn't added, or the same
    /// vertex twice; its index.
    Tri(usize),
    /// Two halfedges running from `a` to `b`.
    Duplicate(u32, u32),
    /// A halfedge from `a` to `b` with none running back.
    Open(u32, u32),
    /// A curve given for vertices no triangle joins.
    UnusedEdge(u32, u32),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::TooManyPatches(n) => write!(f, "{n} triangles are more than {MAX_PATCHES}"),
            BuildError::Tri(t) => write!(
                f,
                "triangle {t} names a missing or repeated vertex, or a missing face"
            ),
            BuildError::Duplicate(a, b) => write!(f, "two halfedges run from vertex {a} to {b}"),
            BuildError::Open(a, b) => write!(f, "no halfedge runs back from vertex {b} to {a}"),
            BuildError::UnusedEdge(a, b) => write!(f, "no triangle joins vertices {a} and {b}"),
        }
    }
}

impl std::error::Error for BuildError {}

impl MeshBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a vertex at `p`, returning its id.
    pub fn vert(&mut self, p: DVec3) -> u32 {
        self.verts.push(p);
        (self.verts.len() - 1) as u32
    }

    /// Adds a face, returning its id.
    pub fn face(&mut self, face: Face) -> u32 {
        self.faces.push(face);
        (self.faces.len() - 1) as u32
    }

    /// Gives face `face` the alias `key`: see [`Mesh::aliases`].
    pub fn alias(&mut self, face: u32, key: FaceKey) {
        self.aliases.push((face, key));
    }

    /// Makes the edge between vertices `a` and `b` (either way round) the
    /// curve with control point `ctrl` and weight `weight`.
    pub fn edge(&mut self, a: u32, b: u32, ctrl: DVec3, weight: f64) {
        self.curves
            .insert((a.min(b), a.max(b)), Edge { ctrl, weight });
    }

    /// Adds the triangle with `corners`, counter-clockwise seen from
    /// outside, on face `face`.
    pub fn tri(&mut self, corners: [u32; 3], face: u32) {
        self.tris.push((corners, face));
    }

    /// Adds the flat wall swept by moving the straight edge from `a0` to
    /// `a1` to the one from `b0` to `b1`, on `face`: the triangles `(a0,
    /// a1, b1)` and `(a0, b1, b0)`, as [`cylinder_strip`] makes them.
    pub(crate) fn wall(&mut self, [a0, a1]: [u32; 2], [b0, b1]: [u32; 2], face: u32) {
        self.tri([a0, a1, b1], face);
        self.tri([a0, b1, b0], face);
    }

    /// Adds the wall swept by moving `bottom`, the curve from vertex `a[0]`
    /// to `a[1]`, along `offset` to the vertices `b`, on `face`: the two
    /// patches of [`cylinder_strip`], with the bottom, the top and the
    /// diagonal from `a[0]` to `b[1]` made its curves. Walls and caps
    /// sharing those vertices share the curves.
    pub(crate) fn curved_wall(
        &mut self,
        a: [u32; 2],
        b: [u32; 2],
        bottom: &Conic3,
        offset: DVec3,
        face: u32,
    ) -> Result<(), PatchError> {
        let [first, second] = cylinder_strip(bottom, offset)?;
        self.edge(a[0], a[1], bottom.c, bottom.w);
        self.edge(b[0], b[1], second.c[1], second.w[1]);
        self.edge(a[0], b[1], first.c[2], first.w[2]);
        self.wall(a, b, face);
        Ok(())
    }

    /// Adds a strip's two patches, `(a[0], a[1], b[1])` and `(a[0], b[1],
    /// b[0])` (as [`crate::sweep`] makes them), on `face`, with their five
    /// edges made their curves: walls and caps sharing those vertices
    /// share the curves.
    pub fn strip(&mut self, a: [u32; 2], b: [u32; 2], patches: &[Patch; 2], face: u32) {
        let [first, second] = patches;
        self.edge(a[0], a[1], first.c[0], first.w[0]);
        self.edge(a[1], b[1], first.c[1], first.w[1]);
        self.edge(b[1], a[0], first.c[2], first.w[2]);
        self.edge(b[1], b[0], second.c[1], second.w[1]);
        self.edge(b[0], a[0], second.c[2], second.w[2]);
        self.wall(a, b, face);
    }

    /// The mesh, with each halfedge paired to the one running back and
    /// the `Edge` records numbered in the order their first halfedge
    /// comes.
    pub fn build(self) -> Result<Mesh, BuildError> {
        let n = self.tris.len();
        if n > MAX_PATCHES {
            return Err(BuildError::TooManyPatches(n));
        }
        let (nv, nf) = (self.verts.len(), self.faces.len());
        for (t, (c, face)) in self.tris.iter().enumerate() {
            let distinct = c[0] != c[1] && c[1] != c[2] && c[2] != c[0];
            if !distinct || c.iter().any(|&v| v as usize >= nv) || *face as usize >= nf {
                return Err(BuildError::Tri(t));
            }
        }
        // Directed edge (a, b) → halfedge.
        let mut directed = LookupMap::with_capacity_and_hasher(3 * n, Default::default());
        for (t, (c, _)) in self.tris.iter().enumerate() {
            for i in 0..3 {
                let (a, b) = (c[i], c[(i + 1) % 3]);
                if directed.insert((a, b), (3 * t + i) as u32).is_some() {
                    return Err(BuildError::Duplicate(a, b));
                }
            }
        }
        let mut tris = Vec::with_capacity(n);
        let mut edges: Vec<Edge> = Vec::with_capacity(3 * n / 2);
        let mut edge_of = vec![u32::MAX; 3 * n];
        let mut used = 0;
        for (t, &(c, face)) in self.tris.iter().enumerate() {
            let mut halfedges = [Halfedge {
                start: 0,
                pair: 0,
                edge: 0,
            }; 3];
            for i in 0..3 {
                let h = 3 * t + i;
                let (a, b) = (c[i], c[(i + 1) % 3]);
                let pair = *directed.get(&(b, a)).ok_or(BuildError::Open(a, b))?;
                if edge_of[pair as usize] == u32::MAX {
                    let key = (a.min(b), a.max(b));
                    let edge = match self.curves.get(&key) {
                        Some(&curve) => {
                            used += 1;
                            curve
                        }
                        None => Edge::straight(self.verts[a as usize], self.verts[b as usize]),
                    };
                    edge_of[h] = edges.len() as u32;
                    edges.push(edge);
                } else {
                    edge_of[h] = edge_of[pair as usize];
                }
                halfedges[i] = Halfedge {
                    start: a,
                    pair,
                    edge: edge_of[h],
                };
            }
            tris.push(Tri { halfedges, face });
        }
        if used != self.curves.len() {
            let unused = self
                .curves
                .keys()
                .find(|&&(a, b)| !directed.contains_key(&(a, b)));
            let &(a, b) = unused.expect("a curve no triangle uses");
            return Err(BuildError::UnusedEdge(a, b));
        }
        Ok(Mesh::from_parts(self.verts, edges, tris, self.faces).with_aliases(self.aliases))
    }
}

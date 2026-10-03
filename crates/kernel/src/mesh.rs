//! Closed meshes of rational quadratic patches.
//!
//! A [`Mesh`] is a halfedge mesh: vertices, a table of shared [`Edge`]s
//! (the middle control point and weight of each edge curve, stored once
//! for the two halfedges that run along it, so neighbours trace the same
//! curve by construction), triangles of three halfedges, and the
//! [`Face`]s the triangles belong to. Each triangle is a [`Patch`] whose
//! corners are its halfedges' start vertices and whose edges are their
//! `Edge` records. Beside the faces, each face's aliases: the keys of
//! faces merged into it ([`Mesh::aliases`]).
//!
//! A face as users see it (a [`FaceKey`]) is an edge-connected region of
//! one surface: booleans and extrude name adjacent faces on one plane or
//! quadric alike once they are repaired (`merge_faces`, in `merge.rs`).
//!
//! [`Mesh::check`] is where a mesh becomes trusted. It covers:
//!
//! 1. Topology: every halfedge has a pair running the other way, directed
//!    edges are unique, and every vertex has one fan: a closed, oriented
//!    2-manifold; aliases name faces that exist.
//! 2. Shared edges: a halfedge and its pair name the same `Edge`, each
//!    `Edge` is used by exactly one pair, and coordinates and weights are
//!    within the patch bounds.
//! 3. Fold: every patch passes the fold check.
//! 4. Control hulls: patches that share no vertex have hulls more than the
//!    resolution apart, and neighbours are split by a plane through what
//!    they share (or, across a curved edge, by the cylinder over it).
//! 5. Orientation: every shell (connected part) faces out, or in where it
//!    bounds a void, so the winding number is 0 or 1 everywhere.
//! 6. Face tags: patches on `Plane` and `Quadric` faces lie on them within
//!    the resolution, checked in every build.
//!
//! Debug builds also check each face's [`Form`], what surface it was meant
//! to be, against its patches within the fit tolerance.
//!
//! [`Mesh::repair`] restores invariants 3 and 4 by exact red–green
//! refinement, and [`Mesh::cuboid`] and [`Mesh::cylinder`] build boxes and
//! cylinders that pass them all.
//!
//! The rules and the reasons for them are written down in
//! `agents/kernel.md`.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, DefaultHasher};

use glam::DVec3;

use crate::patch::{Conic3, Patch};

mod build;
mod bvh;
mod check;
mod face;
mod form;
mod hull;
mod merge;
mod orient;
mod primitive;
mod refine;
mod repair;
mod shape;

pub use build::{BuildError, MeshBuilder};
pub use bvh::Bvh;
pub use check::CheckError;
pub(crate) use check::{off_surface, samples};
pub use face::{Face, FaceKey, FaceName, FacePart, PartKey, Quadric, Surface};
pub use form::Form;
pub(crate) use form::{circle_of, hypot};
pub(crate) use hull::{apart, edge_neighbours_apart, flat, straight};
pub(crate) use refine::{Node, Refiner};
pub(crate) use repair::{MIN_CURVED_SPLIT, MIN_SPLIT, Refusal};
pub(crate) use shape::{SIN_SHAPE, circumcentre_from};

/// A hash map with a fixed hasher, for maps only looked up in, never
/// iterated: nothing can then depend on its order, and lookups by vertex
/// ids beat a `BTreeMap`'s on large meshes.
pub(crate) type LookupMap<K, V> = HashMap<K, V, BuildHasherDefault<DefaultHasher>>;

/// A closed mesh of rational quadratic patches. Halfedge `h` is corner
/// `h % 3` of triangle `h / 3`, running from that corner to the next.
///
/// Nothing is trusted until [`Mesh::check`] passes: the accessors that
/// follow indices ([`Mesh::patch`], [`Mesh::end`]) may panic on a mesh
/// that doesn't.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mesh {
    verts: Vec<DVec3>,
    edges: Vec<Edge>,
    tris: Vec<Tri>,
    faces: Vec<Face>,
    /// Each face's aliases, by face: sorted, without repeats, none a key
    /// of the face's own name.
    aliases: Vec<(u32, FaceKey)>,
}

/// The middle of an edge curve: its control point and weight, shared by
/// the two halfedges that run along it. The curve needs no direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Edge {
    pub ctrl: DVec3,
    pub weight: f64,
}

impl Edge {
    /// The middle of `conic`.
    pub(crate) fn of(conic: &Conic3) -> Edge {
        Edge {
            ctrl: conic.c,
            weight: conic.w,
        }
    }

    /// The straight edge between `a` and `b`: the control point at the
    /// midpoint and weight 1.
    pub fn straight(a: DVec3, b: DVec3) -> Edge {
        Edge {
            ctrl: (a + b) * 0.5,
            weight: 1.0,
        }
    }
}

/// One side of an edge, in a triangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Halfedge {
    /// The vertex it starts at. It ends where the triangle's next
    /// halfedge starts.
    pub start: u32,
    /// The halfedge running the other way along the same edge.
    pub pair: u32,
    /// The shared [`Edge`] it runs along.
    pub edge: u32,
}

/// A triangle: halfedge `i` runs from corner `i` to corner `i + 1`,
/// counter-clockwise seen from outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tri {
    pub halfedges: [Halfedge; 3],
    /// The [`Face`] it is part of.
    pub face: u32,
}

impl Mesh {
    /// The mesh made of these parts, unchecked: see [`Mesh::check`].
    /// [`MeshBuilder`] pairs the halfedges up from a list of triangles.
    pub fn from_parts(
        verts: Vec<DVec3>,
        edges: Vec<Edge>,
        tris: Vec<Tri>,
        faces: Vec<Face>,
    ) -> Self {
        Mesh {
            verts,
            edges,
            tris,
            faces,
            aliases: Vec::new(),
        }
    }

    /// The mesh with `aliases` as its faces' aliases (see
    /// [`Mesh::aliases`]): sorted, repeats and a face's own key dropped.
    /// [`Mesh::check`] refuses one naming a face that doesn't exist.
    pub fn with_aliases(mut self, mut aliases: Vec<(u32, FaceKey)>) -> Self {
        let faces = &self.faces;
        aliases.retain(|&(f, key)| {
            faces
                .get(f as usize)
                .is_none_or(|face| face.name.key() != key)
        });
        aliases.sort_unstable();
        aliases.dedup();
        self.aliases = aliases;
        self
    }

    pub fn verts(&self) -> &[DVec3] {
        &self.verts
    }

    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    pub fn tris(&self) -> &[Tri] {
        &self.tris
    }

    pub fn faces(&self) -> &[Face] {
        &self.faces
    }

    /// The keys of faces merged into each face (by face index, sorted):
    /// a key absorbed when two faces on one surface became one still
    /// names the face that took it in, so a reference to it resolves
    /// there ([`Topology`](crate::topology::Topology)). Operations carry
    /// them as they carry names.
    pub fn aliases(&self) -> &[(u32, FaceKey)] {
        &self.aliases
    }

    /// The aliases of face `face`: see [`Mesh::aliases`].
    pub fn face_aliases(&self, face: u32) -> impl Iterator<Item = FaceKey> + '_ {
        let from = self.aliases.partition_point(|&(f, _)| f < face);
        self.aliases[from..]
            .iter()
            .take_while(move |&&(f, _)| f == face)
            .map(|&(_, key)| key)
    }

    /// Whether the mesh has no triangles: the empty solid.
    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    /// Halfedge `h`: corner `h % 3` of triangle `h / 3`.
    pub fn halfedge(&self, h: u32) -> Halfedge {
        self.tris[h as usize / 3].halfedges[h as usize % 3]
    }

    /// The halfedge after `h` in its triangle.
    pub fn next(h: u32) -> u32 {
        if h % 3 == 2 { h - 2 } else { h + 1 }
    }

    /// The vertex halfedge `h` ends at.
    pub fn end(&self, h: u32) -> u32 {
        self.halfedge(Self::next(h)).start
    }

    /// The curve of halfedge `h`, from its start to its end.
    pub fn curve(&self, h: u32) -> Conic3 {
        let he = self.halfedge(h);
        let edge = self.edges[he.edge as usize];
        Conic3 {
            p0: self.verts[he.start as usize],
            c: edge.ctrl,
            w: edge.weight,
            p1: self.verts[self.end(h) as usize],
        }
    }

    /// Triangle `tri` as a patch. The indices must be in range.
    pub fn patch(&self, tri: usize) -> Patch {
        let hs = self.tris[tri].halfedges;
        let edge = |i: usize| self.edges[hs[i].edge as usize];
        Patch {
            p: hs.map(|h| self.verts[h.start as usize]),
            c: [0, 1, 2].map(|i| edge(i).ctrl),
            w: [0, 1, 2].map(|i| edge(i).weight),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;

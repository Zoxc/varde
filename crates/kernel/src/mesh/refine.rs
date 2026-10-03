//! Red–green refinement of a patch mesh.
//!
//! The mesh is kept as a set of **leaves**, the patches of the input and
//! the pieces red splits (4-way, [`Patch::split4`]) made of them, which
//! need not be conforming: a leaf may have a neighbour one level finer
//! across one of its edges, whose split left a midpoint on it (a hanging
//! vertex). What the mesh is made of, its **pieces**, is conforming: a leaf
//! with no hanging vertex is one piece, and a leaf with one is two green
//! pieces, bisected at it ([`Patch::bisect_with`]). The rules that keep it
//! so:
//!
//! - Before a leaf is split, every coarser neighbour is (so levels across
//!   an edge differ by at most one).
//! - A leaf left with two or three hanging vertices is split too.
//! - So is a leaf on a plane face left with one whose straight bisection
//!   (below) would turn a piece inside out, where it can be split.
//!
//! Splitting a green piece splits its leaf, so green pieces are never
//! bisected again: every piece is a red descendant of an input patch, or
//! half of one, and pieces don't get thinner the deeper they go.
//!
//! Every split is at `½`, so an edge is split once and both sides read the
//! same records: its halves and midpoint are made the first time a leaf on
//! either side splits and stored by their ends. Nothing is matched by
//! position.
//!
//! Patches on a [`Surface::Plane`] face are split with straight inner
//! edges. The pieces cover exactly the region the parent did, the plane
//! being the plane, as long as no straight inner edge crosses a curved
//! side; where one does (a triangle with two or three curved sides, one
//! concave), a child's corner at a curve's midpoint turns inside out, the
//! pieces cover it only up to sign, and splitting further keeps the fold
//! (repair then refuses the mesh). A green piece's straight inner edge
//! can leave its leaf too: from the end of a side bulging into the leaf
//! (a hole's rim on a cap) to the middle of the side opposite, where that
//! midpoint lies beyond the curve's tangent at the end. Such a leaf, if
//! it passes the fold check itself, is split red instead (its children
//! tested the same way when they get a hanging vertex): red inner edges
//! join midpoints, which with one curved side keeps every piece passing
//! it. Neighbours inside a flat face stay separable by a plane through
//! their shared edge, which an exact split's curved inner edges, lying in
//! the face's plane with both pieces, would not be.
//! That is only right for a patch that is on its plane, so the tag isn't
//! trusted: an input patch whose control points aren't all within the
//! resolution of it ([`on_surface`], the test [`Mesh::check_faces`]
//! makes) fails the split with [`CheckError::Face`] naming it, rather
//! than be reshaped. Its straight pieces keep their control points in its
//! hull, so deeper leaves aren't tested again (rounding could fail them
//! at exactly the resolution).

use glam::DVec3;

use super::check::on_surface;
use super::{CheckError, Edge, Face, FaceKey, LookupMap, Mesh, MeshBuilder, Surface};
use crate::budget::Work;
use crate::patch::{Conic3, Patch};
use crate::{KernelError, MAX_PATCHES, MAX_REFINE_DEPTH};

/// An undirected edge by its end vertices, the smaller first.
type Key = (u32, u32);

fn key(a: u32, b: u32) -> Key {
    (a.min(b), a.max(b))
}

/// A patch of the input or of a red split.
#[derive(Debug, Clone)]
struct Leaf {
    corners: [u32; 3],
    face: u32,
    /// The input triangle it came from.
    origin: u32,
    /// How many red splits it is from the input.
    level: u32,
    patch: Patch,
    /// Whether it was made, or given a hanging vertex, since
    /// [`Refiner::settle`].
    changed: bool,
}

/// A patch the refinement has made or started from: an input patch, or
/// a red child of one, with its parent's id (`u32::MAX` for an input
/// patch). Ids are leaf ids: a leaf is a node not split (yet).
#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub corners: [u32; 3],
    pub patch: Patch,
    pub parent: u32,
}

/// A triangle of the conforming mesh: a whole leaf, or half of one.
#[derive(Debug, Clone)]
pub(crate) struct Piece {
    pub corners: [u32; 3],
    pub patch: Patch,
    pub face: u32,
    /// The leaf it is, or is half of.
    pub leaf: u32,
    /// The input triangle it came from.
    pub origin: u32,
    /// Whether it is new since [`Refiner::settle`].
    pub changed: bool,
}

/// A mesh under red–green refinement: see the [module](self) docs.
#[derive(Debug)]
pub(crate) struct Refiner<'a> {
    faces: &'a [Face],
    /// The input's aliases, which its faces keep.
    aliases: &'a [(u32, FaceKey)],
    /// The resolution input patches on a [`Surface::Plane`] face are
    /// tested against before they are split as planar.
    resolution: f64,
    /// Leaves whose control points span less than this along every axis
    /// aren't split.
    min_size: f64,
    /// The most leaves there may be: [`MAX_PATCHES`], as every leaf is
    /// at least one piece.
    max_leaves: usize,
    /// How many leaves there are (the `Some`s of `leaves`).
    live: usize,
    verts: Vec<DVec3>,
    /// Leaves by id; `None` once split.
    leaves: Vec<Option<Leaf>>,
    /// Every node ever a leaf, by id.
    nodes: Vec<Node>,
    /// Every edge record by its ends, including whole edges since split
    /// (a coarser leaf still has the whole edge as its side).
    edges: LookupMap<Key, Edge>,
    /// The midpoint vertex of each split edge.
    mids: LookupMap<Key, u32>,
    /// The whole edge each half was split from.
    halves: LookupMap<Key, Key>,
    /// The leaf with each directed edge, as its side.
    owner: LookupMap<(u32, u32), u32>,
}

impl<'a> Refiner<'a> {
    /// Every triangle of `mesh`, which passes the topology check, as a
    /// leaf at level 0, with the triangle's index as its id. Leaves whose
    /// control points span less than `min_size` along every axis won't be
    /// split. `resolution` is the tolerance's, for the plane tags.
    pub(crate) fn new(mesh: &'a Mesh, resolution: f64, min_size: f64) -> Self {
        let mut refiner = Refiner {
            faces: &mesh.faces,
            aliases: &mesh.aliases,
            resolution,
            min_size,
            max_leaves: MAX_PATCHES,
            live: mesh.tris.len(),
            verts: mesh.verts.clone(),
            leaves: Vec::with_capacity(mesh.tris.len()),
            nodes: Vec::with_capacity(mesh.tris.len()),
            edges: LookupMap::default(),
            mids: LookupMap::default(),
            halves: LookupMap::default(),
            owner: LookupMap::default(),
        };
        for (t, tri) in mesh.tris.iter().enumerate() {
            let corners = tri.halfedges.map(|h| h.start);
            for (i, h) in tri.halfedges.iter().enumerate() {
                let (a, b) = (corners[i], corners[(i + 1) % 3]);
                refiner.edges.insert(key(a, b), mesh.edges[h.edge as usize]);
                refiner.owner.insert((a, b), t as u32);
            }
            let patch = mesh.patch(t);
            refiner.nodes.push(Node {
                corners,
                patch,
                parent: u32::MAX,
            });
            refiner.leaves.push(Some(Leaf {
                corners,
                face: tri.face,
                origin: t as u32,
                level: 0,
                patch,
                changed: false,
            }));
        }
        refiner
    }

    /// Splits the leaves `requested` (ids of leaves, which must exist, in
    /// the order given) and whatever the rules above take with them.
    /// Each split takes a unit of `work`, and so does each plane leaf
    /// tested for a bisection that would fold. Fails with
    /// [`KernelError::TooComplex`] rather than split a leaf that is
    /// [`MAX_REFINE_DEPTH`] levels deep or too small, or make more than
    /// [`MAX_PATCHES`] leaves: the pieces would be more still, so the
    /// round would fail anyway, and stopping here bounds what a round
    /// holds before it is counted (a round could otherwise split every
    /// leaf of a mesh just under the limit, and more by the rules above).
    pub(crate) fn split(&mut self, requested: &[u32], work: &mut Work) -> Result<(), KernelError> {
        let mut stack = Vec::new();
        for &t in requested {
            stack.push(t);
            while let Some(&t) = stack.last() {
                let Some(leaf) = &self.leaves[t as usize] else {
                    stack.pop();
                    continue;
                };
                let corners = leaf.corners;
                if let Some(coarser) = self.coarser_neighbour(corners) {
                    stack.push(coarser);
                    continue;
                }
                stack.pop();
                work.spend(1)?;
                self.split_leaf(t)?;
                for i in 0..3 {
                    let (a, b) = (corners[i], corners[(i + 1) % 3]);
                    // A neighbour at the same level now has a hanging
                    // vertex on this edge.
                    if let Some(&n) = self.owner.get(&(b, a)) {
                        let leaf = self.leaves[n as usize].as_mut().expect("owners are leaves");
                        leaf.changed = true;
                        let hanging = self.hanging(self.leaf(n).corners).count();
                        if hanging >= 2 || (hanging == 1 && self.bisector_folds(n, work)?) {
                            stack.push(n);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Whether leaf `t`, with one hanging vertex, is a plane leaf whose
    /// straight bisection ([`Self::pieces`]) would have a piece fail the
    /// fold check where the leaf passes it, and which can be split
    /// instead (not [`MAX_REFINE_DEPTH`] levels deep, not too small, and
    /// with room for three more leaves). A piece that can't be built
    /// counts as folding. Takes a unit of `work` for a leaf that gets as
    /// far as the fold checks. A leaf whose plane tag is wrong isn't split
    /// for it: that is left to [`Self::pieces`] to name.
    fn bisector_folds(&self, t: u32, work: &mut Work) -> Result<bool, KernelError> {
        let leaf = self.leaf(t);
        let mut hanging = self.hanging(leaf.corners);
        let Some((i, m)) = hanging.next() else {
            return Ok(false);
        };
        if hanging.next().is_some() || !matches!(self.planar(leaf), Ok(true)) {
            return Ok(false);
        }
        let bounds = leaf.patch.bounds();
        let small = (bounds.max - bounds.min).max_element() < self.min_size;
        if leaf.level >= MAX_REFINE_DEPTH || small || self.live + 3 > self.max_leaves {
            return Ok(false);
        }
        work.spend(1)?;
        if leaf.patch.fold_direction().is_none() {
            return Ok(false);
        }
        let [a, b, o] = [0, 1, 2].map(|k| leaf.corners[(i + k) % 3]);
        let folds = |corners| {
            self.patch_at(corners, Some((m, o)))
                .map_or(true, |piece| piece.fold_direction().is_none())
        };
        Ok(folds([a, m, o]) || folds([m, b, o]))
    }

    fn leaf(&self, t: u32) -> &Leaf {
        self.leaves[t as usize].as_ref().expect("a leaf")
    }

    /// The patch of leaf `t`, which must exist.
    pub(crate) fn leaf_patch(&self, t: u32) -> &Patch {
        &self.leaf(t).patch
    }

    /// The edges `(i, midpoint)` of a leaf with `corners` that have a
    /// hanging vertex.
    fn hanging(&self, corners: [u32; 3]) -> impl Iterator<Item = (usize, u32)> + '_ {
        (0..3).filter_map(move |i| {
            let k = key(corners[i], corners[(i + 1) % 3]);
            self.mids.get(&k).map(|&m| (i, m))
        })
    }

    /// A leaf one level coarser than the leaf with `corners` across one of
    /// its edges: one whose side that edge is half of.
    fn coarser_neighbour(&self, corners: [u32; 3]) -> Option<u32> {
        for i in 0..3 {
            let (a, b) = (corners[i], corners[(i + 1) % 3]);
            // Same level across it, or finer (it is split).
            if self.owner.contains_key(&(b, a)) || self.mids.contains_key(&key(a, b)) {
                continue;
            }
            let (x, y) = self.halves[&key(a, b)];
            let coarser = self.owner.get(&(x, y)).or_else(|| self.owner.get(&(y, x)));
            return Some(*coarser.expect("the whole edge has a leaf"));
        }
        None
    }

    /// Whether `leaf` is split with straight inner edges: whether it is on
    /// a [`Surface::Plane`] face. An input leaf (level 0) that isn't on
    /// that plane within the resolution fails with [`CheckError::Face`]
    /// naming its triangle; see the [module](self) docs.
    fn planar(&self, leaf: &Leaf) -> Result<bool, KernelError> {
        let surface = &self.faces[leaf.face as usize].surface;
        if !matches!(surface, Surface::Plane { .. }) {
            return Ok(false);
        }
        if leaf.level == 0 && !on_surface(&leaf.patch, surface, self.resolution) {
            return Err(KernelError::Invalid(CheckError::Face(leaf.origin)));
        }
        Ok(true)
    }

    /// The conic from vertex `a` to vertex `b` along their edge record.
    fn conic(&self, a: u32, b: u32) -> Conic3 {
        let edge = self.edges[&key(a, b)];
        Conic3 {
            p0: self.verts[a as usize],
            c: edge.ctrl,
            w: edge.weight,
            p1: self.verts[b as usize],
        }
    }

    /// The midpoint vertex of the edge from `a` to `b`, splitting it at
    /// `½` first if no leaf on either side has.
    fn split_edge(&mut self, a: u32, b: u32) -> Result<u32, KernelError> {
        let k = key(a, b);
        if let Some(&m) = self.mids.get(&k) {
            return Ok(m);
        }
        let halves = self.conic(k.0, k.1).split_half()?;
        let m = u32::try_from(self.verts.len()).map_err(|_| KernelError::TooComplex)?;
        self.verts.push(halves[0].p1);
        for (half, (x, y)) in halves.iter().zip([(k.0, m), (m, k.1)]) {
            let edge = Edge {
                ctrl: half.c,
                weight: half.w,
            };
            self.edges.insert(key(x, y), edge);
            self.halves.insert(key(x, y), k);
        }
        self.mids.insert(k, m);
        Ok(m)
    }

    /// The patch with `corners` whose edges are the records by their
    /// ends, except that the edge between the vertices `straight`, if
    /// given, is straight (it has no record).
    fn patch_at(
        &self,
        corners: [u32; 3],
        straight: Option<(u32, u32)>,
    ) -> Result<Patch, KernelError> {
        let straight = straight.map(|(a, b)| key(a, b));
        let edge = |i: usize| {
            let (a, b) = (corners[i], corners[(i + 1) % 3]);
            if straight == Some(key(a, b)) {
                Edge::straight(self.verts[a as usize], self.verts[b as usize])
            } else {
                self.edges[&key(a, b)]
            }
        };
        let e = [edge(0), edge(1), edge(2)];
        Ok(Patch::new(
            corners.map(|v| self.verts[v as usize]),
            e.map(|e| e.ctrl),
            e.map(|e| e.weight),
        )?)
    }

    /// Replaces leaf `t` by its four red children.
    fn split_leaf(&mut self, t: u32) -> Result<(), KernelError> {
        let leaf = self.leaf(t).clone();
        // First, so a wrong tag is named whatever else stops the split.
        let planar = self.planar(&leaf)?;
        let bounds = leaf.patch.bounds();
        let small = (bounds.max - bounds.min).max_element() < self.min_size;
        if leaf.level >= MAX_REFINE_DEPTH || small || self.live + 3 > self.max_leaves {
            return Err(KernelError::TooComplex);
        }
        self.live += 3;
        // The exact children, for their inner edges; a planar leaf's are
        // straight.
        let exact = if planar {
            None
        } else {
            Some(leaf.patch.split4()?)
        };
        let [v0, v1, v2] = leaf.corners;
        let m01 = self.split_edge(v0, v1)?;
        let m12 = self.split_edge(v1, v2)?;
        let m20 = self.split_edge(v2, v0)?;
        let inner = [(m01, m12), (m12, m20), (m20, m01)];
        for (i, (a, b)) in inner.into_iter().enumerate() {
            let edge = match &exact {
                // The middle child runs m01 → m12 → m20.
                Some(kids) => Edge {
                    ctrl: kids[3].c[i],
                    weight: kids[3].w[i],
                },
                None => Edge::straight(self.verts[a as usize], self.verts[b as usize]),
            };
            self.edges.insert(key(a, b), edge);
        }
        for i in 0..3 {
            self.owner
                .remove(&(leaf.corners[i], leaf.corners[(i + 1) % 3]));
        }
        let children = [
            [v0, m01, m20],
            [m01, v1, m12],
            [m20, m12, v2],
            [m01, m12, m20],
        ];
        self.leaves[t as usize] = None;
        for (c, corners) in children.into_iter().enumerate() {
            let patch = self.patch_at(corners, None)?;
            if let Some(kids) = &exact {
                // The records are the ones the split made, whichever side
                // split each edge first.
                debug_assert_eq!(patch, kids[c], "a red child differs from its split");
            }
            let id = u32::try_from(self.leaves.len()).map_err(|_| KernelError::TooComplex)?;
            for i in 0..3 {
                self.owner.insert((corners[i], corners[(i + 1) % 3]), id);
            }
            self.nodes.push(Node {
                corners,
                patch,
                parent: t,
            });
            self.leaves.push(Some(Leaf {
                corners,
                face: leaf.face,
                origin: leaf.origin,
                level: leaf.level + 1,
                patch,
                changed: true,
            }));
        }
        Ok(())
    }

    /// The conforming mesh's triangles, leaf by leaf in id order.
    pub(crate) fn pieces(&self) -> Result<Vec<Piece>, KernelError> {
        let mut pieces = Vec::with_capacity(self.leaves.len());
        for (id, leaf) in self.leaves.iter().enumerate() {
            let Some(leaf) = leaf else { continue };
            let piece = |corners, patch| Piece {
                corners,
                patch,
                face: leaf.face,
                leaf: id as u32,
                origin: leaf.origin,
                changed: leaf.changed,
            };
            let mut hanging = self.hanging(leaf.corners);
            let Some((i, m)) = hanging.next() else {
                pieces.push(piece(leaf.corners, leaf.patch));
                continue;
            };
            assert!(
                hanging.next().is_none(),
                "refinement left a leaf with two hanging vertices"
            );
            let [a, b, o] = [0, 1, 2].map(|k| leaf.corners[(i + k) % 3]);
            let halves = [self.conic(a, m), self.conic(m, b)];
            let [first, second] = if self.planar(leaf)? {
                // Joined to the opposite corner by a straight edge.
                let straight = |x: [u32; 3]| self.patch_at(x, Some((m, o)));
                [straight([a, m, o])?, straight([m, b, o])?]
            } else {
                leaf.patch.bisect_with(i, 0.5, halves)?
            };
            pieces.push(piece([a, m, o], first));
            pieces.push(piece([m, b, o], second));
        }
        Ok(pieces)
    }

    /// Every node the refinement has had, by id: the tree of red splits.
    pub(crate) fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// Marks every leaf unchanged.
    pub(super) fn settle(&mut self) {
        for leaf in self.leaves.iter_mut().flatten() {
            leaf.changed = false;
        }
    }

    /// The mesh made of `pieces` (from [`Self::pieces`]), with the input's
    /// faces and their aliases, and every vertex made.
    pub(crate) fn mesh(&self, pieces: &[Piece]) -> Mesh {
        let mut builder = MeshBuilder::new();
        for &v in &self.verts {
            builder.vert(v);
        }
        for &face in self.faces {
            builder.face(face);
        }
        for &(face, key) in self.aliases {
            builder.alias(face, key);
        }
        for piece in pieces {
            let c = piece.corners;
            for i in 0..3 {
                let (ctrl, w) = (piece.patch.c[i], piece.patch.w[i]);
                builder.edge(c[i], c[(i + 1) % 3], ctrl, w);
            }
            builder.tri(c, piece.face);
        }
        builder
            .build()
            .expect("refinement keeps the halfedges paired")
    }
}

#[cfg(test)]
mod tests;

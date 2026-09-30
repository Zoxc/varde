//! An operand of a boolean, with the tables the counting reads: its
//! triangles' corners, its edges' ends and the triangles on each side,
//! and which of its edges are straight and which patches flat or planar.

use glam::DVec3;

use crate::budget::Work;
use crate::mesh::{Mesh, straight};
use crate::patch::{Bounds3, Conic3, Patch, smallest_cone};
use crate::{KernelError, Tolerance};

/// The most different triangle normals round a vertex among which the
/// smallest cone is looked for (it takes the fourth power of their
/// number).
const CONE_NORMALS: usize = 16;

/// Which operand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Side {
    A,
    B,
}

impl Side {
    pub(super) fn other(self) -> Side {
        match self {
            Side::A => Side::B,
            Side::B => Side::A,
        }
    }
}

/// One operand, checked, as tables by index.
#[derive(Debug)]
pub(super) struct Input<'a> {
    pub(super) mesh: &'a Mesh,
    /// Each triangle's corners, counter-clockwise from outside.
    pub(super) tris: Vec<[u32; 3]>,
    /// Each edge record's ends, in the direction of the lower of its two
    /// halfedges: the edge's own direction.
    pub(super) edges: Vec<[u32; 2]>,
    /// Each triangle's edges in order (edge `i` from corner `i` to corner
    /// `i + 1`), and whether the triangle runs along the edge's direction.
    pub(super) tri_edges: Vec<[(u32, bool); 3]>,
    /// Each edge's triangles: the one running along it, then the one
    /// running against it.
    pub(super) edge_tris: Vec<[u32; 2]>,
    /// The box around each patch's control points.
    pub(super) boxes: Vec<Bounds3>,
    /// Each triangle's patch.
    pub(super) patches: Vec<Patch>,
    /// Whether each edge is straight within the resolution: then it is
    /// taken as the segment between its ends, and decided exactly.
    pub(super) straight: Vec<bool>,
    /// Whether each patch's edges are all straight: then it is taken as
    /// the triangle on its corners, and decided exactly.
    pub(super) flat: Vec<bool>,
    /// Whether each patch lies in the plane of its corners within the
    /// resolution (flat patches do, and so do caps with curved edges):
    /// then which side of it a point is on is decided exactly, against
    /// that plane.
    pub(super) planar: Vec<bool>,
    /// Whether any patch isn't flat.
    pub(super) curved: bool,
}

impl<'a> Input<'a> {
    /// The tables of `mesh`, which must pass `check` (a [`Solid`]'s mesh
    /// does), with straightness and flatness judged within `tol`'s
    /// resolution.
    ///
    /// [`Solid`]: crate::Solid
    pub(super) fn new(mesh: &'a Mesh, tol: &Tolerance) -> Input<'a> {
        let margin = tol.resolution();
        let n = mesh.tris().len();
        let mut tris = Vec::with_capacity(n);
        let mut edges = vec![[0u32; 2]; mesh.edges().len()];
        let mut edge_tris = vec![[0u32; 2]; mesh.edges().len()];
        let mut tri_edges = Vec::with_capacity(n);
        let mut boxes = Vec::with_capacity(n);
        let mut patches = Vec::with_capacity(n);
        for (t, tri) in mesh.tris().iter().enumerate() {
            let patch = mesh.patch(t);
            boxes.push(patch.bounds());
            patches.push(patch);
            let hs = tri.halfedges;
            tris.push(hs.map(|h| h.start));
            let mut te = [(0u32, true); 3];
            for i in 0..3 {
                let h = (3 * t + i) as u32;
                let e = hs[i].edge;
                let forward = h < hs[i].pair;
                te[i] = (e, forward);
                if forward {
                    edges[e as usize] = [hs[i].start, hs[(i + 1) % 3].start];
                    edge_tris[e as usize][0] = t as u32;
                } else {
                    edge_tris[e as usize][1] = t as u32;
                }
            }
            tri_edges.push(te);
        }
        let straight: Vec<bool> = edges
            .iter()
            .zip(mesh.edges())
            .map(|(&[s, e], edge)| {
                let [p, q] = [s, e].map(|v| mesh.verts()[v as usize]);
                straight(p, edge.ctrl, q, margin)
            })
            .collect();
        let flat: Vec<bool> = tri_edges
            .iter()
            .map(|te| te.iter().all(|&(e, _)| straight[e as usize]))
            .collect();
        let planar: Vec<bool> = patches
            .iter()
            .zip(&flat)
            .map(|(patch, &flat)| flat || planar(patch, margin))
            .collect();
        let curved = flat.iter().any(|&f| !f);
        Input {
            mesh,
            tris,
            edges,
            tri_edges,
            edge_tris,
            boxes,
            patches,
            straight,
            flat,
            planar,
            curved,
        }
    }

    /// Edge `e` as a curve, in its own direction.
    pub(super) fn conic(&self, e: u32) -> Conic3 {
        let [s, t] = self.edges[e as usize];
        let edge = self.mesh.edges()[e as usize];
        Conic3 {
            p0: self.pos(s),
            c: edge.ctrl,
            w: edge.weight,
            p1: self.pos(t),
        }
    }

    pub(super) fn pos(&self, v: u32) -> DVec3 {
        self.mesh.verts()[v as usize]
    }

    /// Triangle `t`'s corners' positions.
    pub(super) fn corners(&self, t: u32) -> [DVec3; 3] {
        self.tris[t as usize].map(|v| self.pos(v))
    }

    /// The face triangle `t` is on.
    pub(super) fn face(&self, t: u32) -> u32 {
        self.mesh.tris()[t as usize].face
    }

    /// The volume enclosed by the triangles on the corners, summed in
    /// triangle order: positive for a flat solid facing out.
    pub(super) fn volume(&self) -> f64 {
        let Some(&o) = self.mesh.verts().first() else {
            return 0.0;
        };
        (0..self.tris.len() as u32)
            .map(|t| {
                let [a, b, c] = self.corners(t).map(|p| p - o);
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }

    /// Whether the solid faces out: its volume is positive. For flat
    /// patches, the corner triangles' ([`Self::volume`]). With curved ones,
    /// the same, less the volume between each patch and its triangle,
    /// integrated ([`crate::solid::patch_volume`], `per_patch` work each)
    /// for the patches that could move it most until what the rest could
    /// no longer changes the sign: each patch lies in its control points'
    /// hull (its weights are positive), as do its corner triangle and the
    /// lunes between its curved edges and their chords (which the two
    /// patches beside an edge share, turned opposite ways), so the volume
    /// between the two is no more than the hull's, itself no more than
    /// twice the control points' distance from the triangle's plane times
    /// the square of their spread (twice that again, for safety). The
    /// whole solid's integral took 0.8 s on a body of 47 000 patches,
    /// before any work was counted; most of them are small and flat
    /// enough to leave out.
    pub(super) fn faces_out(&self, per_patch: usize, work: &mut Work) -> Result<bool, KernelError> {
        if !self.curved {
            return Ok(self.volume() > 0.0);
        }
        work.spend(self.patches.len())?;
        let Some(&o) = self.mesh.verts().first() else {
            return Ok(false);
        };
        let tet = |t: usize| {
            let [a, b, c] = self.corners(t as u32).map(|p| p - o);
            a.dot(b.cross(c)) / 6.0
        };
        let bound = |patch: &Patch| {
            let [p0, p1, p2] = patch.p;
            let Some(n) = (p1 - p0).cross(p2 - p0).try_normalize() else {
                return f64::INFINITY;
            };
            let off = patch
                .c
                .iter()
                .map(|&c| (c - p0).dot(n).abs())
                .fold(0.0, f64::max);
            let points = [p0, p1, p2, patch.c[0], patch.c[1], patch.c[2]];
            let spread = points
                .iter()
                .flat_map(|&x| points.iter().map(move |&y| x.distance_squared(y)))
                .fold(0.0, f64::max);
            4.0 * off * spread
        };
        let mut v: f64 = (0..self.tris.len()).map(tet).sum();
        let mut order: Vec<(f64, usize)> = self
            .patches
            .iter()
            .enumerate()
            .map(|(t, patch)| (bound(patch), t))
            .filter(|&(b, _)| b > 0.0)
            .collect();
        order.sort_by(|x, y| y.0.total_cmp(&x.0).then(x.1.cmp(&y.1)));
        // What the patches from each on could still move it by.
        let mut rest = vec![0.0f64; order.len() + 1];
        for k in (0..order.len()).rev() {
            rest[k] = rest[k + 1] + order[k].0;
        }
        for (k, &(_, t)) in order.iter().enumerate() {
            if v.abs() > rest[k] {
                break;
            }
            work.spend(per_patch)?;
            v += crate::solid::patch_volume(&self.patches[t], o) - tet(t);
        }
        Ok(v > 0.0)
    }

    /// Each vertex's direction out of the solid: one that leaves by
    /// every triangle round the vertex (on the outer side of each one's
    /// plane, a curved patch's normal at the corner standing in for its
    /// triangle's) wherever there is one, so that moving the vertices
    /// along them moves every face outwards. The sum of the triangles' unit
    /// normals, normalized, when it does; else the axis of the smallest
    /// cone round their normals (for up to [`CONE_NORMALS`] different
    /// ones), which does whenever any direction does. Where none does (a
    /// saddle), the sum: it only perturbs ties, and any direction keeps
    /// the operands real.
    pub(super) fn vertex_normals(&self) -> Vec<DVec3> {
        let mut around: Vec<Vec<DVec3>> = vec![Vec::new(); self.mesh.verts().len()];
        for t in 0..self.tris.len() as u32 {
            let [a, b, c] = self.corners(t);
            let flat = (b - a).cross(c - a).normalize_or_zero();
            for (k, v) in self.tris[t as usize].into_iter().enumerate() {
                // A curved patch's own normal at the corner.
                let n = if self.flat[t as usize] {
                    flat
                } else {
                    self.patches[t as usize]
                        .normal(DVec3::AXES[k])
                        .try_normalize()
                        .unwrap_or(flat)
                };
                around[v as usize].push(n);
            }
        }
        around
            .into_iter()
            .map(|mut normals| {
                let sum = normals.iter().copied().sum::<DVec3>().try_normalize();
                let leaves = |d: DVec3| normals.iter().all(|n| n.dot(d) > 0.0);
                if let Some(d) = sum
                    && leaves(d)
                {
                    return d;
                }
                normals.sort_by(|a, b| a.to_array().partial_cmp(&b.to_array()).expect("finite"));
                normals.dedup();
                if (1..=CONE_NORMALS).contains(&normals.len()) {
                    let (axis, least) = smallest_cone(&normals);
                    if least > 0.0 {
                        return axis;
                    }
                }
                sum.unwrap_or(DVec3::Z)
            })
            .collect()
    }
}

/// Whether `patch`'s control points all lie within `margin` of the plane
/// through its corners (which aren't on one line).
fn planar(patch: &Patch, margin: f64) -> bool {
    let [p0, p1, p2] = patch.p;
    let Some(n) = (p1 - p0).cross(p2 - p0).try_normalize() else {
        return false;
    };
    patch.c.iter().all(|&c| n.dot(c - p0).abs() <= margin)
}

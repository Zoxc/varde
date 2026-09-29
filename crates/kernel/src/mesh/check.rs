use super::hull::{edge_neighbours_apart, non_neighbours_apart, vertex_neighbours_apart};
use super::{Bvh, Mesh};
use crate::par::par_map;
use crate::patch::{Patch, PatchError};
use crate::{MAX_PATCHES, Tolerance};

/// Which invariant a mesh breaks, and where: the first failure found, in
/// the order of the invariants, then of halfedge, vertex, edge or triangle
/// index (pairs of triangles in lexicographic order), so the same mesh
/// always gives the same error.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CheckError {
    /// More than [`MAX_PATCHES`] triangles.
    TooManyPatches(usize),
    /// More vertices than halfedges, a number of edges other than half the
    /// halfedges, or more than [`MAX_PATCHES`] faces.
    Counts,
    /// Halfedge `h` names a vertex, pair or edge that doesn't exist, or its
    /// triangle a face that doesn't.
    Index(u32),
    /// Halfedge `h`'s pair isn't paired back with it, or doesn't run the
    /// other way between the same vertices.
    Pair(u32),
    /// Halfedge `h` starts and ends at the same vertex.
    Loop(u32),
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
    /// Triangles that share no vertex have hulls within the resolution.
    Hull(u32, u32),
    /// Triangles sharing an edge aren't split by a plane through it.
    EdgeNeighbours(u32, u32),
    /// Triangles sharing only a vertex aren't split by a plane through it.
    VertexNeighbours(u32, u32),
    /// Triangles sharing all three corners.
    SameCorners(u32, u32),
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
            CheckError::DirectedEdge(h) => write!(f, "halfedge {h} runs the same way as another"),
            CheckError::Fan(v) => write!(f, "vertex {v} doesn't have exactly one fan"),
            CheckError::SharedEdge(h) => {
                write!(f, "halfedge {h} and its pair name different edges")
            }
            CheckError::EdgeUse(e) => write!(f, "edge {e} isn't used by exactly one pair"),
            CheckError::Patch(t, e) => write!(f, "patch {t}: {e}"),
            CheckError::Fold(t) => write!(f, "patch {t} may fold"),
            CheckError::Face(t) => write!(f, "patch {t} is off its face's surface"),
            CheckError::Hull(a, b) => write!(f, "the hulls of patches {a} and {b} come too close"),
            CheckError::EdgeNeighbours(a, b) => {
                write!(f, "patches {a} and {b} aren't split by their shared edge")
            }
            CheckError::VertexNeighbours(a, b) => {
                write!(f, "patches {a} and {b} aren't split at their shared vertex")
            }
            CheckError::SameCorners(a, b) => write!(f, "patches {a} and {b} share all corners"),
        }
    }
}

impl std::error::Error for CheckError {}

impl Mesh {
    /// Checks every invariant (see the [module](super) docs): topology,
    /// shared edges, folds, control hulls with `tol`'s resolution as the
    /// margin, and in debug builds face tags ([`Self::check_faces`]).
    /// The empty mesh passes.
    pub fn check(&self, tol: &Tolerance) -> Result<(), CheckError> {
        self.check_topology()?;
        let patches = self.checked_patches()?;
        if cfg!(debug_assertions) {
            self.check_faces_of(&patches, tol)?;
        }
        self.check_hulls(&patches, tol)
    }

    /// Invariants 1 and 2 without the geometry: pairs, directed edges,
    /// fans, and shared edge records.
    pub(super) fn check_topology(&self) -> Result<(), CheckError> {
        let nt = self.tris.len();
        if nt > MAX_PATCHES {
            return Err(CheckError::TooManyPatches(nt));
        }
        let nh = 3 * nt;
        if self.verts.len() > nh || 2 * self.edges.len() != nh || self.faces.len() > MAX_PATCHES {
            return Err(CheckError::Counts);
        }
        let (nv, ne, nf) = (self.verts.len(), self.edges.len(), self.faces.len());
        for (t, tri) in self.tris.iter().enumerate() {
            for (i, he) in tri.halfedges.iter().enumerate() {
                let bad = he.start as usize >= nv
                    || he.pair as usize >= nh
                    || he.edge as usize >= ne
                    || tri.face as usize >= nf;
                if bad {
                    return Err(CheckError::Index((3 * t + i) as u32));
                }
            }
        }
        let nh = nh as u32;
        for h in 0..nh {
            let he = self.halfedge(h);
            let pair = self.halfedge(he.pair);
            let (start, end) = (he.start, self.end(h));
            if he.pair == h || pair.pair != h || pair.start != end || self.end(he.pair) != start {
                return Err(CheckError::Pair(h));
            }
            if start == end {
                return Err(CheckError::Loop(h));
            }
            if pair.edge != he.edge {
                return Err(CheckError::SharedEdge(h));
            }
        }
        let mut uses = vec![0u8; ne];
        for h in 0..nh {
            let e = self.halfedge(h).edge as usize;
            uses[e] = uses[e].saturating_add(1);
        }
        if let Some(e) = uses.iter().position(|&n| n != 2) {
            return Err(CheckError::EdgeUse(e as u32));
        }
        let mut directed: Vec<(u32, u32, u32)> = (0..nh)
            .map(|h| (self.halfedge(h).start, self.end(h), h))
            .collect();
        directed.sort_unstable();
        // Sorted by halfedge within equal directed edges, so the first of
        // each window is the lower.
        let repeated = directed
            .windows(2)
            .filter(|w| (w[0].0, w[0].1) == (w[1].0, w[1].1))
            .map(|w| w[0].2)
            .min();
        if let Some(h) = repeated {
            return Err(CheckError::DirectedEdge(h));
        }
        // Every vertex starts halfedges, and walking round it from the
        // first, `next(pair(h))`, visits them all before coming back.
        let mut out = vec![0u32; nv];
        let mut first = vec![u32::MAX; nv];
        for h in 0..nh {
            let v = self.halfedge(h).start as usize;
            out[v] += 1;
            first[v] = first[v].min(h);
        }
        for v in 0..nv {
            if out[v] == 0 {
                return Err(CheckError::Fan(v as u32));
            }
            let mut h = first[v];
            let mut steps = 0;
            loop {
                h = Self::next(self.halfedge(h).pair);
                steps += 1;
                if h == first[v] || steps > out[v] {
                    break;
                }
            }
            if steps != out[v] {
                return Err(CheckError::Fan(v as u32));
            }
        }
        Ok(())
    }

    /// Every triangle as a patch within the coordinate and weight bounds
    /// (the rest of invariant 2) that passes the fold check (invariant 3).
    fn checked_patches(&self) -> Result<Vec<Patch>, CheckError> {
        let tris: Vec<u32> = (0..self.tris.len() as u32).collect();
        let checked = par_map(&tris, |&t| {
            let patch = self.patch(t as usize);
            patch.check().map_err(|e| CheckError::Patch(t, e))?;
            match patch.fold_direction() {
                Some(_) => Ok(patch),
                None => Err(CheckError::Fold(t)),
            }
        });
        checked.into_iter().collect()
    }

    /// Invariant 5: every patch on a `Plane` face has its six control
    /// points within `tol`'s resolution of the plane, and every patch on
    /// a `Quadric` face has sampled points within it of the quadric (to
    /// first order). [`Self::check`] runs this in debug builds only; call
    /// it after `check` passes.
    pub fn check_faces(&self, tol: &Tolerance) -> Result<(), CheckError> {
        let patches: Vec<Patch> = (0..self.tris.len()).map(|t| self.patch(t)).collect();
        self.check_faces_of(&patches, tol)
    }

    fn check_faces_of(&self, patches: &[Patch], tol: &Tolerance) -> Result<(), CheckError> {
        use super::Surface;
        let resolution = tol.resolution();
        let tris: Vec<u32> = (0..self.tris.len() as u32).collect();
        let on = par_map(&tris, |&t| {
            let patch = &patches[t as usize];
            let surface = self.faces[self.tris[t as usize].face as usize].surface;
            // Written so that NaN fails.
            let near = |x| surface.distance(x) <= resolution;
            let ok = match surface {
                Surface::Free => true,
                Surface::Plane { .. } => patch.hull().into_iter().all(near),
                Surface::Quadric(_) => samples().all(|u| near(patch.eval(u))),
            };
            if ok { Ok(()) } else { Err(CheckError::Face(t)) }
        });
        on.into_iter().collect()
    }

    /// Invariant 4 over every pair of patches whose boxes come within the
    /// resolution, from the BVH. Pairs that share vertices always do,
    /// sharing control points.
    fn check_hulls(&self, patches: &[Patch], tol: &Tolerance) -> Result<(), CheckError> {
        let margin = tol.resolution();
        let bvh = Bvh::new(patches.iter().map(Patch::bounds).collect());
        let pairs = bvh.self_pairs(margin);
        let results = par_map(&pairs, |&[i, j]| {
            let (a, b) = (&patches[i as usize], &patches[j as usize]);
            check_pair([i, j], [a, b], [self.corners(i), self.corners(j)], margin)
        });
        results.into_iter().collect()
    }

    /// The vertex ids at the corners of triangle `t`.
    pub(super) fn corners(&self, t: u32) -> [u32; 3] {
        self.tris[t as usize].halfedges.map(|h| h.start)
    }
}

/// Invariant 4 for triangles `ids`, the patches `patches` with corners at
/// the vertex ids `corners`, whose boxes come within `margin`. The rule is
/// chosen by how many vertices they share, which is topology.
pub(super) fn check_pair(
    ids: [u32; 2],
    [a, b]: [&Patch; 2],
    [ca, cb]: [[u32; 3]; 2],
    margin: f64,
) -> Result<(), CheckError> {
    let [i, j] = ids;
    // (corner of a, corner of b) for each shared vertex.
    let mut found = [(0, 0); 3];
    let mut count = 0;
    for (k, &corner) in ca.iter().enumerate() {
        if let Some(l) = cb.iter().position(|&v| v == corner) {
            found[count] = (k, l);
            count += 1;
        }
    }
    let shared = &found[..count];
    let ok = match *shared {
        [] => non_neighbours_apart(a, b, margin),
        [(ka, kb)] => vertex_neighbours_apart(a, ka, b, kb, margin),
        [(k0, l0), (k1, l1)] => {
            // The edge of `a` from one shared corner to the other, and
            // `b`'s, which runs the other way.
            let ea = if (k0 + 1) % 3 == k1 { k0 } else { k1 };
            let eb = if (l1 + 1) % 3 == l0 { l1 } else { l0 };
            edge_neighbours_apart(a, ea, b, eb, margin)
        }
        _ => return Err(CheckError::SameCorners(i, j)),
    };
    if ok {
        Ok(())
    } else {
        Err(match shared.len() {
            0 => CheckError::Hull(i, j),
            1 => CheckError::VertexNeighbours(i, j),
            _ => CheckError::EdgeNeighbours(i, j),
        })
    }
}

/// Barycentric points where face tags are sampled on quadrics: a grid of
/// 15, four steps along each edge.
fn samples() -> impl Iterator<Item = glam::DVec3> {
    (0..=4).flat_map(|i| {
        (0..=4 - i).map(move |j| {
            let (u, v) = (i as f64 / 4.0, j as f64 / 4.0);
            glam::DVec3::new(u, v, 1.0 - u - v)
        })
    })
}

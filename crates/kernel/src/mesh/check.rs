use glam::DVec3;

use super::hull::{edge_neighbours_parted, non_neighbours_apart, vertex_neighbours_apart};
use super::{Bvh, Form, Mesh, Surface};
use crate::budget::Work;
use crate::par::par_map;
use crate::patch::{Patch, PatchError};
use crate::{KernelError, MAX_PATCHES, Tolerance};

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
    /// Alias `i` of [`Mesh::aliases`] names a face that doesn't exist or
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

impl From<CheckError> for KernelError {
    fn from(e: CheckError) -> Self {
        KernelError::Invalid(e)
    }
}

impl Mesh {
    /// Checks every invariant (see the [module](super) docs): topology,
    /// shared edges, folds, control hulls with `tol`'s resolution as the
    /// margin, orientation, and face tags ([`Self::check_faces`]), in
    /// every build. The empty mesh passes.
    pub fn check(&self, tol: &Tolerance) -> Result<(), CheckError> {
        self.check_counted(tol).map(drop)
    }

    /// [`Self::check`], giving how many patches' volumes it integrated to
    /// tell which way the shells face (the rest is linear in the patches
    /// and their hull pairs), for callers that charge the work.
    pub(crate) fn check_counted(&self, tol: &Tolerance) -> Result<usize, CheckError> {
        let (patches, bvh) = self.check_embedded(tol)?;
        self.check_rest(&patches, &bvh, tol)
    }

    /// [`Self::check_counted`], charging `work` what a first pass of
    /// [`repair`](Self::repair_within) over the mesh would: two units a
    /// patch, and one for each pair of patches whose boxes come within
    /// the resolution, counted before they are collected
    /// ([`Bvh::self_pairs_within`]), so boxes crowding each other fail it
    /// with [`KernelError::TooComplex`] rather than make pairs of nearly
    /// every two. A failure of the check is [`KernelError::Invalid`].
    pub(crate) fn check_counted_within(
        &self,
        tol: &Tolerance,
        work: &mut Work,
    ) -> Result<usize, KernelError> {
        let (patches, bvh) = self.check_embedded_by(tol, |bvh, margin| {
            work.spend(self.tris.len().saturating_mul(2))?;
            bvh.self_pairs_within(margin, work)
        })?;
        self.check_rest(&patches, &bvh, tol)
            .map_err(KernelError::Invalid)
    }

    /// The invariants after the hulls: orientation and face tags.
    fn check_rest(
        &self,
        patches: &[Patch],
        bvh: &Bvh,
        tol: &Tolerance,
    ) -> Result<usize, CheckError> {
        let integrated = self.check_orientation(patches, bvh, tol.resolution())?;
        self.check_faces_of(patches, tol)?;
        #[cfg(debug_assertions)]
        if let Some((t, why)) = self.off_forms(patches, tol) {
            panic!("triangle {t} {why}");
        }
        Ok(integrated)
    }

    /// Invariants 1 to 4, returning the patches: everything but the
    /// orientation and the face tags.
    pub(super) fn check_embedding(&self, tol: &Tolerance) -> Result<Vec<Patch>, CheckError> {
        self.check_embedded(tol).map(|(patches, _)| patches)
    }

    /// [`Self::check_embedding`], with the BVH over the patches' boxes.
    pub(super) fn check_embedded(&self, tol: &Tolerance) -> Result<(Vec<Patch>, Bvh), CheckError> {
        self.check_embedded_by(tol, |bvh, margin| Ok(bvh.self_pairs(margin)))
    }

    /// [`Self::check_embedded`], with `pairs` finding the pairs of boxes
    /// within the margin from the BVH (and failing as it says).
    fn check_embedded_by<E: From<CheckError>>(
        &self,
        tol: &Tolerance,
        pairs: impl FnOnce(&Bvh, f64) -> Result<Vec<[u32; 2]>, E>,
    ) -> Result<(Vec<Patch>, Bvh), E> {
        self.check_topology()?;
        let patches = self.bounded_patches()?;
        let folds = par_map(&patches, |patch| patch.fold_direction().is_some());
        if let Some(t) = folds.iter().position(|&passes| !passes) {
            return Err(CheckError::Fold(t as u32).into());
        }
        let margin = tol.resolution();
        let bvh = Bvh::new(patches.iter().map(Patch::bounds).collect());
        let pairs = pairs(&bvh, margin)?;
        self.check_hulls(&patches, &pairs, margin)?;
        Ok((patches, bvh))
    }

    /// Invariants 1 and 2 without the geometry: pairs, directed edges,
    /// fans, and shared edge records.
    pub(crate) fn check_topology(&self) -> Result<(), CheckError> {
        let nt = self.tris.len();
        if nt > MAX_PATCHES {
            return Err(CheckError::TooManyPatches(nt));
        }
        let nh = 3 * nt;
        if self.verts.len() > nh
            || 2 * self.edges.len() != nh
            || self.faces.len() > MAX_PATCHES
            || self.aliases.len() > MAX_PATCHES
        {
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
        for (i, &(face, key)) in self.aliases.iter().enumerate() {
            let bad = self
                .faces
                .get(face as usize)
                .is_none_or(|f| f.name.key() == key)
                || (i > 0 && self.aliases[i - 1] >= (face, key));
            if bad {
                return Err(CheckError::Alias(i as u32));
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

    /// Every triangle as a patch, each within the coordinate and weight
    /// bounds (the rest of invariant 2).
    pub(super) fn bounded_patches(&self) -> Result<Vec<Patch>, CheckError> {
        let tris: Vec<u32> = (0..self.tris.len() as u32).collect();
        let checked = par_map(&tris, |&t| {
            let patch = self.patch(t as usize);
            patch.check().map_err(|e| CheckError::Patch(t, e))?;
            Ok(patch)
        });
        checked.into_iter().collect()
    }

    /// Invariant 6: every patch on a `Plane` face has its six control
    /// points within `tol`'s resolution of the plane, every patch on a
    /// `Quadric` face has sampled points within it of the quadric (to
    /// first order), and every patch whose face claims a plane or has a
    /// plane [`Form`] faces along that plane's normal at its
    /// middle (the normal points out of the solid, for a face claiming no
    /// surface too). [`Self::check`] runs this last; on its own, call it
    /// on a mesh that passes the rest of `check` (orientation aside).
    pub fn check_faces(&self, tol: &Tolerance) -> Result<(), CheckError> {
        let patches: Vec<Patch> = (0..self.tris.len()).map(|t| self.patch(t)).collect();
        self.check_faces_of(&patches, tol)
    }

    fn check_faces_of(&self, patches: &[Patch], tol: &Tolerance) -> Result<(), CheckError> {
        let resolution = tol.resolution();
        let tris: Vec<u32> = (0..self.tris.len() as u32).collect();
        let on = par_map(&tris, |&t| {
            let face = &self.faces[self.tris[t as usize].face as usize];
            let patch = &patches[t as usize];
            if !on_surface(patch, &face.surface, resolution) {
                return Err(CheckError::Face(t));
            }
            // The tag's and the form's normals: faces of one operand
            // within a tie of the other's have come out of booleans with
            // a triangle of one carrying the other's face, turned against
            // it, on faces that claim a plane and on copies that claim
            // none (and keep the plane form).
            let tag = match face.surface {
                Surface::Plane { n, .. } => Some(n),
                _ => None,
            };
            let form = match face.form {
                Form::Plane { n, .. } => Some(n),
                _ => None,
            };
            if tag.is_none() && form.is_none() {
                return Ok(());
            }
            let normal = patch.normal(DVec3::splat(1.0 / 3.0));
            // NaN is against.
            let along = |n: DVec3| normal.dot(n) > 0.0;
            if tag.into_iter().chain(form).all(along) {
                Ok(())
            } else {
                Err(CheckError::FacesAgainst(t))
            }
        });
        on.into_iter().collect()
    }

    /// Debug builds' check of the faces' forms on a mesh that passes the
    /// rest of `check`: the first triangle with a sample ([`samples`])
    /// further than `tol`'s fit tolerance, times the face's
    /// [`slack`](super::Face::slack), from its face's
    /// [`Form`] (fitted faces are on theirs only that
    /// closely), and what is wrong with it. A form is intent the
    /// construction promises, so one that doesn't hold is a bug, not a
    /// bad input. (Which way a plane form faces is checked in every
    /// build, [`Self::check_faces`]: booleans can get it wrong near ties.)
    #[cfg(debug_assertions)]
    pub(crate) fn off_forms(
        &self,
        patches: &[Patch],
        tol: &Tolerance,
    ) -> Option<(u32, &'static str)> {
        let fit = tol.fit();
        let tris: Vec<u32> = (0..self.tris.len() as u32).collect();
        let off = par_map(&tris, |&t| {
            let face = &self.faces[self.tris[t as usize].face as usize];
            // NaN fails.
            let within = |d: f64| d <= fit * face.slack;
            let patch = &patches[t as usize];
            samples()
                .any(|u| !within(face.form.distance(patch.eval(u))))
                .then_some("strays from its face's form")
        });
        off.iter()
            .enumerate()
            .find_map(|(t, why)| why.map(|why| (t as u32, why)))
    }

    /// Invariant 4 over `pairs`, every pair of patches whose boxes come
    /// within `margin`, the resolution. Pairs that share vertices always
    /// do, sharing control points.
    fn check_hulls(
        &self,
        patches: &[Patch],
        pairs: &[[u32; 2]],
        margin: f64,
    ) -> Result<(), CheckError> {
        let results = par_map(pairs, |&[i, j]| {
            let (a, b) = (&patches[i as usize], &patches[j as usize]);
            check_pair([i, j], [a, b], [self.corners(i), self.corners(j)], margin)
        });
        results.into_iter().collect()
    }

    /// The vertex ids at the corners of triangle `t`.
    pub(crate) fn corners(&self, t: u32) -> [u32; 3] {
        self.tris[t as usize].halfedges.map(|h| h.start)
    }
}

/// Invariant 4 for the patches `a` and `b`, with corners at the vertex
/// ids `ca` and `cb`, whose boxes come within `margin`; an error names
/// them `i` and `j`. The rule is chosen by how many vertices they share,
/// which is topology.
pub(super) fn check_pair(
    [i, j]: [u32; 2],
    [a, b]: [&Patch; 2],
    [ca, cb]: [[u32; 3]; 2],
    margin: f64,
) -> Result<(), CheckError> {
    match ca.iter().filter(|v| cb.contains(v)).count() {
        0 => non_neighbours_apart(a, b, margin)
            .then_some(())
            .ok_or(CheckError::Hull(i, j)),
        1 => {
            let ka = (0..3)
                .find(|&k| cb.contains(&ca[k]))
                .expect("a shared corner");
            let kb = (0..3).find(|&k| cb[k] == ca[ka]).expect("a shared corner");
            vertex_neighbours_apart(a, ka, b, kb, margin)
                .then_some(())
                .ok_or(CheckError::VertexNeighbours(i, j))
        }
        2 => {
            // The edge of each running between the two shared corners;
            // the topology check has them run it opposite ways.
            let edge = |x: [u32; 3], y: [u32; 3]| {
                (0..3)
                    .find(|&k| y.contains(&x[k]) && y.contains(&x[(k + 1) % 3]))
                    .expect("an edge between shared corners")
            };
            edge_neighbours_parted(a, edge(ca, cb), b, edge(cb, ca), margin)
                .then_some(())
                .ok_or(CheckError::EdgeNeighbours(i, j))
        }
        _ => Err(CheckError::SameCorners(i, j)),
    }
}

/// Whether `patch` lies on `surface` as a face tag claims (see
/// [`Mesh::check_faces`]): a plane's patch with its six control points
/// within `resolution` of it, a quadric's with its [`samples`].
pub(crate) fn on_surface(patch: &Patch, surface: &Surface, resolution: f64) -> bool {
    off_surface(patch, surface) <= resolution
}

/// How far `patch` strays from `surface`, as [`on_surface`] measures it:
/// the farthest of a plane's patch's six control points (which bound it),
/// or of a quadric's patch's [`samples`]; NaN counted as infinite, so it
/// fails every bound. 0 for [`Surface::Free`].
pub(crate) fn off_surface(patch: &Patch, surface: &Surface) -> f64 {
    let far = |x| {
        let d = surface.distance(x);
        if d.is_nan() { f64::INFINITY } else { d }
    };
    match surface {
        Surface::Free => 0.0,
        Surface::Plane { .. } => patch.hull().into_iter().map(far).fold(0.0, f64::max),
        Surface::Quadric(_) => samples().map(|u| far(patch.eval(u))).fold(0.0, f64::max),
    }
}

/// Barycentric points where face tags are sampled on quadrics: a grid of
/// 15, four steps along each edge.
pub(crate) fn samples() -> impl Iterator<Item = DVec3> {
    (0..=4).flat_map(|i| {
        (0..=4 - i).map(move |j| {
            let (u, v) = (i as f64 / 4.0, j as f64 / 4.0);
            DVec3::new(u, v, 1.0 - u - v)
        })
    })
}

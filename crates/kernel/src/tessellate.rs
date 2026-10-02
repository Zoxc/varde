//! Tessellating a checked patch mesh into a [`RenderMesh`].
//!
//! Every [`Edge`](crate::mesh::Edge) record gets a number of segments from
//! its own curve alone, and its sample points are worked out once, along
//! the curve's first halfedge, and read by both patches beside it, so
//! neighbours share their boundary points to the bit and the mesh has no
//! cracks. Each patch is sampled on a regular grid inside, one step in
//! from its boundary, and strips of triangles join that grid to the
//! points of its three edges. The grid has as many steps as the patch's
//! finest edge, more on patches curved both ways (spheres, ellipsoids,
//! tori, revolved conics) where that leaves a triangle farther than the
//! chord from the patch: measured and refined in rounds before anything
//! is made, so the counts are still known up front. Normals are the
//! patches' own, shared across an edge where the two sides agree within
//! [`Display::SMOOTH_DEGREES`] and split where they don't; splits and the
//! boundaries between faces of different keys
//! ([`FaceName::key`](crate::mesh::FaceName::key)) are the feature edges
//! drawn with the mesh. The triangles go face by face, a face being a
//! region of the solid's [`Topology`], and the feature edges are
//! polylines from corner to corner: first the topology's chains, then the
//! creases inside one region. The other edges are the wires, drawn only
//! in a wireframe, if they fit within the edges' points beside them.
//!
//! The rules and reasons are written down in `agents/kernel.md`.

use glam::DVec3;

use crate::manifold::{ManifoldError, ManifoldMesh};
use crate::mesh::{Form, Mesh};
use crate::par::par_map;
use crate::patch::{Bounds3, Conic3, Patch};
use crate::render_mesh::split;
use crate::{MeshError, MeshParts, RenderMesh, Tolerance, Topology};

/// How finely [`Solid::tessellate`](crate::Solid::tessellate) samples:
/// each edge curve is cut into segments whose chords are at most
/// [`Display::chord`] from the curve and turn at most
/// [`Display::MAX_TURN_DEGREES`] from one to the next, with at most
/// [`Display::MAX_SEGMENTS`] segments to an edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Display {
    fit: f64,
}

impl Display {
    /// The chord error allowed relative to the solid's diagonal.
    pub const RELATIVE_CHORD: f64 = 1e-3;
    /// The most one segment of an edge may turn, in degrees.
    pub const MAX_TURN_DEGREES: f64 = 10.0;
    /// The most segments an edge is cut into.
    pub const MAX_SEGMENTS: u32 = 64;
    /// The most two patches' normals may differ along an edge they share
    /// for the edge to be drawn smooth, in degrees.
    pub const SMOOTH_DEGREES: f64 = 1.0;

    /// Sampling to within the fit tolerance of `tol` at best.
    pub fn new(tol: &Tolerance) -> Display {
        Display { fit: tol.fit() }
    }

    /// The chord error allowed on a solid `diagonal` across: its
    /// [`Display::RELATIVE_CHORD`], but no less than the fit tolerance.
    pub fn chord(&self, diagonal: f64) -> f64 {
        self.fit.max(Self::RELATIVE_CHORD * diagonal)
    }
}

/// A loose patch sampled for drawing ([`Display::sample_patch`]): its
/// points, their unit normals, and its triangles as indices into them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PatchSamples {
    pub points: Vec<DVec3>,
    pub normals: Vec<DVec3>,
    pub indices: Vec<u32>,
}

impl Display {
    /// `curve` cut as an edge of a solid `diagonal` across is drawn
    /// ([`Display::chord`], [`Display::MAX_TURN_DEGREES`],
    /// [`Display::MAX_SEGMENTS`]): its points at equal parameter steps,
    /// both ends included. Geometry that isn't a solid's, such as a
    /// failure's [`Evidence`](crate::Evidence), is drawn this way.
    pub fn flatten(&self, curve: &Conic3, diagonal: f64) -> Vec<DVec3> {
        let n = segments(curve, self.chord(diagonal));
        (0..=n)
            .map(|s| curve.eval(f64::from(s) / f64::from(n)))
            .collect()
    }

    /// `patch`, on its own, sampled as a patch of a solid `diagonal`
    /// across is drawn: each edge cut as [`Display::flatten`] cuts it
    /// (corners twice, once for each edge), the inside on the grid those
    /// counts give, refined as a face of no known form is (in rounds,
    /// until within the chord or at the finest grid). Nothing is shared
    /// with another patch.
    pub fn sample_patch(&self, patch: &Patch, diagonal: f64) -> PatchSamples {
        let chord = self.chord(diagonal);
        let counts = [0, 1, 2].map(|i| segments(&patch.edge(i), chord));
        let mut level = Level::new(counts);
        let mut round = 0;
        while let Some(steps) = finer(level.steps(), level_error(patch, &level), chord, round) {
            level = Level::with_steps(counts, steps);
            round += 1;
        }
        let (params, first, base) = level.params();
        let points: Vec<DVec3> = params.iter().map(|&u| patch.eval(u)).collect();
        let indices = level.triangulate(patch, base, &points[base as usize..], |i, r| {
            let v = first[i as usize] + r;
            (v, points[v as usize])
        });
        let normals = params.iter().map(|&u| unit_normal(patch, u)).collect();
        PatchSamples {
            points,
            normals,
            indices,
        }
    }
}

impl Default for Display {
    /// At the default [`Tolerance`].
    fn default() -> Self {
        Display::new(&Tolerance::DEFAULT)
    }
}

/// `cos` of [`Display::MAX_TURN_DEGREES`], written out so no platform's
/// `cos` decides a segment count.
const COS_TURN: f64 = 0.984807753012208;
/// [`Display::MAX_TURN_DEGREES`] in radians.
const TURN: f64 = 0.17453292519943295;
/// `cos` of [`Display::SMOOTH_DEGREES`].
const COS_SMOOTH: f64 = 0.9998476951563913;

/// The most a tessellation may make of each part of a [`RenderMesh`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Limits {
    pub(crate) vertices: u64,
    pub(crate) indices: u64,
    pub(crate) edge_points: u64,
}

impl Limits {
    /// [`ManifoldMesh::MAX_VERTICES`] and three indices per
    /// [`ManifoldMesh::MAX_TRIANGLES`]; no edges are drawn.
    pub(crate) const EXPORT: Limits = Limits {
        vertices: ManifoldMesh::MAX_VERTICES as u64,
        indices: 3 * ManifoldMesh::MAX_TRIANGLES as u64,
        edge_points: 0,
    };

    /// [`RenderMesh::MAX_VERTICES`] and the others.
    pub(crate) const RENDER: Limits = Limits {
        vertices: RenderMesh::MAX_VERTICES as u64,
        indices: RenderMesh::MAX_INDICES as u64,
        edge_points: RenderMesh::MAX_EDGE_POINTS as u64,
    };
}

/// `mesh`, which must pass [`Mesh::check`], as triangles: see the module
/// docs. The result is one part, even if it's empty.
pub(crate) fn tessellate(mesh: &Mesh, display: &Display) -> Result<RenderMesh, MeshError> {
    tessellate_within(mesh, display, &Limits::RENDER)
}

/// [`tessellate`] with `mesh`'s topology already worked out.
pub(crate) fn tessellate_with(
    mesh: &Mesh,
    display: &Display,
    topology: &Topology,
) -> Result<RenderMesh, MeshError> {
    draw(mesh, display, &Limits::RENDER, topology)
}

/// What a tessellation of a mesh is made of, worked out from the edges'
/// segment counts and, for patches curved both ways, by measuring their
/// grids ([`Plan::refine`]), before any vertex is made: the drawn
/// ([`tessellate`]) and the welded ([`weld`]) tessellations share it, so
/// they have the same samples and triangles.
struct Plan<'a> {
    mesh: &'a Mesh,
    halfedges: u32,
    /// Each edge's first halfedge (the lowest), the canonical direction
    /// its samples run in.
    first: Vec<u32>,
    /// Each edge's curve along its first halfedge.
    curves: Vec<Conic3>,
    /// Each edge's segment count.
    counts: Vec<u32>,
    tri_ids: Vec<u32>,
    edge_ids: Vec<u32>,
    levels: Vec<Level>,
    /// Where each patch's inner points start among all of them.
    inner: Vec<u64>,
    inner_total: u64,
    triangles: u64,
}

impl<'a> Plan<'a> {
    /// The plan for `mesh`, which must pass [`Mesh::check`], or `None` for
    /// the empty mesh. Refining stops once the plan is past `limits`
    /// (see [`Plan::fits`]).
    fn new(
        mesh: &'a Mesh,
        display: &Display,
        limits: &Limits,
    ) -> Result<Option<Plan<'a>>, MeshError> {
        let Some(bounds) = Bounds3::around(mesh.verts()) else {
            return Ok(None);
        };
        if mesh.is_empty() {
            return Ok(None);
        }
        let chord = display.chord((bounds.max - bounds.min).length());
        let halfedges = u32::try_from(mesh.tris().len() * 3).map_err(|_| MeshError::TooLarge)?;

        // A checked mesh uses every edge twice.
        let mut first = vec![u32::MAX; mesh.edges().len()];
        for h in 0..halfedges {
            let e = mesh.halfedge(h).edge as usize;
            if first[e] == u32::MAX {
                first[e] = h;
            }
        }
        let curves: Vec<Conic3> = first.iter().map(|&h| mesh.curve(h)).collect();
        let counts: Vec<u32> = par_map(&curves, |c| segments(c, chord));
        let n_of = |h: u32| counts[mesh.halfedge(h).edge as usize];

        let tri_ids: Vec<u32> = (0..halfedges / 3).collect();
        let levels: Vec<Level> = tri_ids
            .iter()
            .map(|&t| Level::new([0, 1, 2].map(|i| n_of(3 * t + i))))
            .collect();
        let edge_ids = (0..mesh.edges().len() as u32).collect();
        let mut plan = Plan {
            mesh,
            halfedges,
            first,
            curves,
            counts,
            tri_ids,
            edge_ids,
            levels,
            inner: Vec::new(),
            inner_total: 0,
            triangles: 0,
        };
        plan.count();
        plan.refine(chord, limits);
        Ok(Some(plan))
    }

    /// Works out the triangles and inner points from the levels.
    fn count(&mut self) {
        self.triangles = 0;
        self.inner.clear();
        self.inner_total = 0;
        for level in &self.levels {
            self.triangles = self.triangles.saturating_add(level.triangles());
            self.inner.push(self.inner_total);
            self.inner_total = self.inner_total.saturating_add(level.inner_points());
        }
    }

    /// Makes the inner grids of the patches curved both ways (see
    /// [`curved_both_ways`]) finer, in rounds, until each one's triangles
    /// are within `chord` of it ([`level_error`]) or its grid has
    /// [`MAX_INNER_STEPS`] (a single triangle too far gets a grid of 3
    /// steps, one inner point, first): each round measures the patches
    /// still open at their levels and moves those too far to a grid of
    /// `√(error / chord)` times the steps, at least one more in the first
    /// [`FINE_ROUNDS`] and a quarter more after, so there are at most
    /// about two dozen rounds. Edge counts, and so the shared samples,
    /// stay as they are. Stops as soon as the plan
    /// no longer fits `limits`, so a round's work is bounded by them (the
    /// caller then refuses the plan); the levels depend only on the mesh
    /// and `chord`, whatever the limits, when it fits.
    fn refine(&mut self, chord: f64, limits: &Limits) {
        let mesh = self.mesh;
        let form = |t: u32| &mesh.faces()[mesh.tris()[t as usize].face as usize].form;
        let mut open: Vec<u32> = (self.tri_ids.iter().copied())
            .filter(|&t| curved_both_ways(form(t)))
            .collect();
        let mut round = 0;
        while !open.is_empty() && self.fits(limits) {
            let levels = &self.levels;
            let errors = par_map(&open, |&t| {
                level_error(&mesh.patch(t as usize), &levels[t as usize])
            });
            let mut next = Vec::new();
            for (&t, &error) in open.iter().zip(&errors) {
                let level = &mut self.levels[t as usize];
                let Some(steps) = finer(level.steps(), error, chord, round) else {
                    continue;
                };
                *level = Level::with_steps(level.counts, steps);
                next.push(t);
            }
            self.count();
            open = next;
            round += 1;
        }
    }

    /// Halfedge `h`'s segment count.
    fn n_of(&self, h: u32) -> u32 {
        self.counts[self.mesh.halfedge(h).edge as usize]
    }

    /// The fewest vertices the tessellation has: one per mesh vertex, per
    /// inner sample of an edge and per inner point of a patch. The welded
    /// one has exactly these.
    fn least_vertices(&self) -> u64 {
        let edge_points: u64 = self.counts.iter().map(|&n| u64::from(n - 1)).sum();
        (self.mesh.verts().len() as u64)
            .saturating_add(edge_points)
            .saturating_add(self.inner_total)
    }

    /// Whether the triangles and the fewest vertices fit `limits`.
    fn fits(&self, limits: &Limits) -> bool {
        self.triangles.saturating_mul(3) <= limits.indices
            && self.least_vertices() <= limits.vertices
    }

    /// The barycentric parameters of patch `t`'s inner points, in the
    /// order [`Level::index`] numbers them, or none for a single
    /// triangle.
    fn inner_params(&self, t: u32) -> Vec<DVec3> {
        self.levels[t as usize].inner_params()
    }

    /// Patch `t`'s triangles, as vertex ids: its inner points are `base`
    /// on, at `inner` (as [`Plan::inner_params`] orders them), and
    /// `outer(h, r)` is the id and position of halfedge `h`'s sample `r`;
    /// `patch` is patch `t`.
    fn patch_triangles(
        &self,
        patch: &Patch,
        t: u32,
        base: u32,
        inner: &[DVec3],
        outer: impl Fn(u32, u32) -> (u32, DVec3),
    ) -> Vec<u32> {
        self.levels[t as usize].triangulate(patch, base, inner, |i, r| outer(3 * t + i, r))
    }
}

/// [`tessellate`], failing with [`MeshError::TooLarge`] if the mesh
/// would have more of a part than `limits` allow, which must be within
/// [`Limits::RENDER`]. Each part is counted before it's made.
pub(crate) fn tessellate_within(
    mesh: &Mesh,
    display: &Display,
    limits: &Limits,
) -> Result<RenderMesh, MeshError> {
    draw(mesh, display, limits, &Topology::of(mesh))
}

/// [`tessellate_within`], its faces and chains those of `topology`, which
/// must be `mesh`'s.
fn draw(
    mesh: &Mesh,
    display: &Display,
    limits: &Limits,
    topology: &Topology,
) -> Result<RenderMesh, MeshError> {
    assert_eq!(
        topology.triangles(),
        mesh.tris().len(),
        "the topology is the mesh's"
    );
    let Some(plan) = Plan::new(mesh, display, limits)? else {
        return RenderMesh::from_parts(MeshParts {
            part_ends: vec![[0; 4]],
            ..MeshParts::default()
        });
    };
    if !plan.fits(limits) {
        return Err(MeshError::TooLarge);
    }
    let Plan {
        halfedges,
        first,
        curves,
        counts,
        tri_ids,
        edge_ids,
        inner,
        inner_total,
        triangles,
        ..
    } = &plan;
    let (halfedges, inner_total, triangles) = (*halfedges, *inner_total, *triangles);
    let n_of = |h: u32| plan.n_of(h);

    // Each halfedge's face: its triangle's region.
    let face_of = |h: u32| topology.region_of(h / 3);

    // Unit normals along each halfedge of each patch, at its edge's
    // sample points in the halfedge's own direction.
    let boundary: Vec<Vec<DVec3>> = par_map(tri_ids, |&t| {
        let patch = mesh.patch(t as usize);
        let mut normals = Vec::new();
        for i in 0..3 {
            let n = n_of(3 * t + i as u32);
            for r in 0..=n {
                let s = f64::from(r) / f64::from(n);
                let mut u = DVec3::ZERO;
                u[i] = 1.0 - s;
                u[(i + 1) % 3] = s;
                normals.push(unit_normal(&patch, u));
            }
        }
        normals
    });
    // Halfedge `h`'s normal at its own sample `r`.
    let normal_at = |h: u32, r: u32| {
        let t = h / 3;
        let skip: u32 = (3 * t..h).map(|g| n_of(g) + 1).sum();
        boundary[t as usize][(skip + r) as usize]
    };

    let smooth: Vec<bool> = par_map(edge_ids, |&e| {
        let (a, n) = (first[e as usize], counts[e as usize]);
        let b = mesh.halfedge(a).pair;
        (0..=n).all(|s| normal_at(a, s).dot(normal_at(b, n - s)) >= COS_SMOOTH)
    });
    let feature: Vec<bool> = edge_ids
        .iter()
        .map(|&e| {
            let a = first[e as usize];
            let b = mesh.halfedge(a).pair;
            !smooth[e as usize] || face_of(a) != face_of(b)
        })
        .collect();
    let chains = Chains::new(mesh, first, &feature, face_of, topology);
    // Each polyline has a point more than its segments.
    let feature_points = (chains.halfedges.iter())
        .map(|&h| u64::from(n_of(h)))
        .sum::<u64>()
        .saturating_add(chains.ends.len() as u64);
    if feature_points > limits.edge_points {
        return Err(MeshError::TooLarge);
    }

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    // The `f64` points of the vertices on edges, which the strips choose
    // their diagonals from, as welding does.
    let mut exact: Vec<DVec3> = Vec::new();

    // One vertex per corner group: the corners round a vertex between
    // two split edges, sharing their summed normal.
    let mut out_of = vec![u32::MAX; mesh.verts().len()];
    for h in 0..halfedges {
        let v = mesh.halfedge(h).start as usize;
        if out_of[v] == u32::MAX {
            out_of[v] = h;
        }
    }
    let mut corner_vertex = vec![0u32; halfedges as usize];
    let mut fan = Vec::new();
    let mut members = Vec::new();
    for (v, &start) in out_of.iter().enumerate() {
        if start == u32::MAX {
            continue;
        }
        fan.clear();
        let mut h = start;
        // Consecutive corners of the fan share the edge of the first.
        loop {
            fan.push(h);
            h = Mesh::next(mesh.halfedge(h).pair);
            if h == start || fan.len() >= halfedges as usize {
                break;
            }
        }
        let split = |h: u32| !smooth[mesh.halfedge(h).edge as usize];
        let from = fan.iter().position(|&h| split(h)).map_or(0, |k| k + 1);
        let mut sum = DVec3::ZERO;
        members.clear();
        for step in 0..fan.len() {
            let h = fan[(from + step) % fan.len()];
            members.push(h);
            sum += normal_at(h, 0);
            if split(h) || step + 1 == fan.len() {
                let index = positions.len() as u32;
                let normal = sum.try_normalize().unwrap_or(normal_at(members[0], 0));
                positions.push(mesh.verts()[v].as_vec3().to_array());
                normals.push(normal.as_vec3().to_array());
                exact.push(mesh.verts()[v]);
                for &m in &members {
                    corner_vertex[m as usize] = index;
                }
                sum = DVec3::ZERO;
                members.clear();
            }
        }
    }

    // The inner sample points of each edge: once if it's smooth, with the
    // two sides' mean normal, else once for each side.
    let edge_vertices: Vec<Vec<(DVec3, [f32; 3])>> = par_map(edge_ids, |&e| {
        let (a, n) = (first[e as usize], counts[e as usize]);
        let b = mesh.halfedge(a).pair;
        let curve = &curves[e as usize];
        let points: Vec<DVec3> = (1..n)
            .map(|s| curve.eval(f64::from(s) / f64::from(n)))
            .collect();
        let f32s = |n: DVec3| n.as_vec3().to_array();
        let mut out = Vec::new();
        if smooth[e as usize] {
            for s in 1..n {
                let (na, nb) = (normal_at(a, s), normal_at(b, n - s));
                let mean = (na + nb).try_normalize().unwrap_or(na);
                out.push((points[s as usize - 1], f32s(mean)));
            }
        } else {
            for (side, flip) in [(a, false), (b, true)] {
                for s in 1..n {
                    let r = if flip { n - s } else { s };
                    out.push((points[s as usize - 1], f32s(normal_at(side, r))));
                }
            }
        }
        out
    });
    let mut edge_base = Vec::with_capacity(edge_vertices.len());
    for vertices in &edge_vertices {
        edge_base.push(positions.len() as u64);
        for &(p, n) in vertices {
            positions.push(p.as_vec3().to_array());
            normals.push(n);
            exact.push(p);
        }
    }
    drop(edge_vertices);
    let inner_base = positions.len() as u64;
    if inner_base.saturating_add(inner_total) > limits.vertices {
        return Err(MeshError::TooLarge);
    }

    // Halfedge `h`'s vertex at its own sample `r`.
    let sample_vertex = |h: u32, r: u32| -> u32 {
        let n = n_of(h);
        if r == 0 {
            return corner_vertex[h as usize];
        }
        if r == n {
            return corner_vertex[Mesh::next(h) as usize];
        }
        let e = mesh.halfedge(h).edge as usize;
        let canonical = first[e] == h;
        let s = if canonical { r } else { n - r };
        let side = if smooth[e] || canonical { 0 } else { n - 1 };
        // Within `MAX_VERTICES`, checked above.
        (edge_base[e] + u64::from(side + s - 1)) as u32
    };

    let patches: Vec<(Vec<Vertex>, Vec<u32>)> = par_map(tri_ids, |&t| {
        let patch = mesh.patch(t as usize);
        let params = plan.inner_params(t);
        let inner_points: Vec<DVec3> = params.iter().map(|&u| patch.eval(u)).collect();
        let points: Vec<Vertex> = (params.iter().zip(&inner_points))
            .map(|(&u, p)| {
                (
                    p.as_vec3().to_array(),
                    unit_normal(&patch, u).as_vec3().to_array(),
                )
            })
            .collect();
        let base = (inner_base + inner[t as usize]) as u32;
        let indices = plan.patch_triangles(&patch, t, base, &inner_points, |h, r| {
            let v = sample_vertex(h, r);
            (v, exact[v as usize])
        });
        (points, indices)
    });
    let mut tri_indices = Vec::with_capacity(patches.len());
    for (points, indices) in patches {
        for (p, n) in points {
            positions.push(p);
            normals.push(n);
        }
        tri_indices.push(indices);
    }
    debug_assert_eq!(positions.len() as u64, inner_base + inner_total);

    // The triangles region by region, each region's in the order of its
    // patches.
    let mut indices = Vec::new();
    let mut face_ends = Vec::with_capacity(topology.regions().len());
    for region in topology.regions() {
        for &t in &region.tris {
            indices.extend_from_slice(&tri_indices[t as usize]);
        }
        // Within `MAX_INDICES`, checked above.
        face_ends.push(indices.len() as u32);
    }
    drop(tri_indices);
    debug_assert_eq!(indices.len() as u64, 3 * triangles);

    // Each chain's samples, its joints once. Where a crease ends at a
    // joint, the halfedges either side of it may have their own vertices
    // there (the corner groups split at the crease), at the same position:
    // the one before stands for both, and a closed chain ends on the
    // vertex it starts on.
    let mut edge_vertices = Vec::new();
    let mut edge_ends = Vec::with_capacity(chains.ends.len());
    for (halfedges, &[begin, finish]) in
        split(&chains.halfedges, &chains.ends).zip(&chains.edge_corners)
    {
        let first = sample_vertex(halfedges[0], 0);
        edge_vertices.push(first);
        for &h in halfedges {
            debug_assert_eq!(
                edge_vertices.last().map(|&v| positions[v as usize]),
                Some(positions[sample_vertex(h, 0) as usize]),
            );
            edge_vertices.extend((1..=n_of(h)).map(|r| sample_vertex(h, r)));
        }
        if begin == finish
            && let Some(last) = edge_vertices.last_mut()
        {
            *last = first;
        }
        // Within `MAX_EDGE_POINTS`, checked above.
        edge_ends.push(edge_vertices.len() as u32);
    }
    debug_assert_eq!(edge_vertices.len() as u64, feature_points);

    // The wires, an edge record each, along its first halfedge: none if
    // they'd take the edges' points past the limit, as the model is drawn
    // without them but for a wireframe.
    let wire_points = (edge_ids.iter())
        .filter(|&&e| !feature[e as usize])
        .map(|&e| u64::from(counts[e as usize]) + 1)
        .sum::<u64>();
    let mut wire_vertices = Vec::new();
    let mut wire_ends = Vec::new();
    if feature_points.saturating_add(wire_points) <= limits.edge_points {
        for &e in edge_ids.iter().filter(|&&e| !feature[e as usize]) {
            let h = first[e as usize];
            wire_vertices.extend((0..=n_of(h)).map(|r| sample_vertex(h, r)));
            // Within `MAX_EDGE_POINTS`, checked above.
            wire_ends.push(wire_vertices.len() as u32);
        }
    }
    let corners = (chains.corners.iter())
        .map(|&v| mesh.verts()[v as usize].as_vec3().to_array())
        .collect();

    // Within `MAX_FACES`, `MAX_EDGE_POLYLINES` and `MAX_CORNERS`, as
    // there are fewer faces than triangles, edges and wires than half
    // their points and corners than edges' ends.
    let part_ends = vec![[
        face_ends.len() as u32,
        edge_ends.len() as u32,
        chains.corners.len() as u32,
        wire_ends.len() as u32,
    ]];
    RenderMesh::from_parts(MeshParts {
        positions,
        normals,
        indices,
        face_ends,
        edge_vertices,
        edge_ends,
        edge_faces: chains.faces,
        corners,
        edge_corners: chains.edge_corners,
        wire_vertices,
        wire_ends,
        part_ends,
    })
}

/// The feature edges as polylines: first the [`Topology`]'s chains, in
/// its order, each a maximal run of [`Edge`](crate::mesh::Edge) records
/// between the same two regions; then the creases, the feature edges
/// inside one region, each a maximal run of records end to end through
/// the mesh vertices where exactly two feature edges meet, both creases
/// of the same region. Every mesh vertex a polyline ends at is a corner;
/// a run that closes without one gets a corner where it starts.
///
/// A chain runs along the halfedges on its first region's side. A crease
/// runs along the halfedges on one side of it, which keep to that side:
/// where two records meet, the halfedge starting there on the side of the
/// one ending there. Only the two creases cross the fan round such a
/// vertex, and a corner group (see [`tessellate_within`]) ends only at a
/// split edge, which is a feature edge, so the two halfedges share their
/// vertex there. A chain may run on through a vertex a crease ends at, so
/// its halfedges there may have vertices of their own, at the same
/// position.
struct Chains {
    /// The chains' halfedges, chain after chain, each from its start.
    halfedges: Vec<u32>,
    /// One past each chain's last halfedge.
    ends: Vec<u32>,
    /// Each chain's faces: its halfedges' side, then the other.
    faces: Vec<[u32; 2]>,
    /// The corners' mesh vertices.
    corners: Vec<u32>,
    /// Each chain's corners, where it starts and where it ends.
    edge_corners: Vec<[u32; 2]>,
    /// Each mesh vertex's corner, or `u32::MAX`.
    corner_of: Vec<u32>,
}

impl Chains {
    /// The polylines of the edges of `mesh` that are `feature`, given
    /// each edge's `first` halfedge and each halfedge's face (`face_of`).
    /// Creases start at the lowest corner first, closed ones at their
    /// lowest edge's first halfedge, so they come out the same every time.
    fn new(
        mesh: &Mesh,
        first: &[u32],
        feature: &[bool],
        face_of: impl Fn(u32) -> u32,
        topology: &Topology,
    ) -> Chains {
        let verts = mesh.verts().len();
        let mut chains = Chains {
            halfedges: Vec::new(),
            ends: Vec::new(),
            faces: Vec::new(),
            corners: Vec::new(),
            edge_corners: Vec::new(),
            corner_of: vec![u32::MAX; verts],
        };
        let mut done = vec![false; feature.len()];
        for chain in topology.chains() {
            let (Some(&start), Some(&last)) = (chain.halfedges.first(), chain.halfedges.last())
            else {
                continue;
            };
            for &h in &chain.halfedges {
                done[mesh.halfedge(h).edge as usize] = true;
            }
            chains.halfedges.extend_from_slice(&chain.halfedges);
            chains.push(chain.regions, mesh.halfedge(start).start, mesh.end(last));
        }
        debug_assert!(
            (0..feature.len()).all(|e| !done[e] || feature[e]),
            "every chain's edges are feature edges"
        );

        // The creases.
        let pair_of = |e: u32| {
            let h = first[e as usize];
            let pair = [face_of(h), face_of(mesh.halfedge(h).pair)];
            [pair[0].min(pair[1]), pair[0].max(pair[1])]
        };
        // The feature edges at each vertex, as (edge, whether the edge's
        // first halfedge starts there), in the order of the edges.
        let mut at: Vec<Vec<(u32, bool)>> = vec![Vec::new(); verts];
        for (e, _) in feature.iter().enumerate().filter(|(_, f)| **f) {
            let h = first[e];
            at[mesh.halfedge(h).start as usize].push((e as u32, true));
            at[mesh.end(h) as usize].push((e as u32, false));
        }
        // A crease runs on through a vertex only to another crease of
        // its region: a chain's edges are between two regions.
        let through: Vec<bool> = at
            .iter()
            .map(|ends| matches!(ends[..], [(a, _), (b, _)] if pair_of(a) == pair_of(b)))
            .collect();
        // The halfedge of edge `e` starting at the vertex `start` says it
        // is at.
        let from = |(e, start): (u32, bool)| {
            let h = first[e as usize];
            if start { h } else { mesh.halfedge(h).pair }
        };
        let starts = (0..verts)
            .filter(|&v| !through[v])
            .flat_map(|v| at[v].iter().copied())
            .chain((0..feature.len() as u32).map(|e| (e, true)));
        for (e, start) in starts {
            if !feature[e as usize] || done[e as usize] {
                continue;
            }
            let mut h = from((e, start));
            let begin = mesh.halfedge(h).start;
            let faces = [face_of(h), face_of(mesh.halfedge(h).pair)];
            debug_assert_eq!(faces[0], faces[1], "what isn't on a chain is a crease");
            let mut edge = e;
            let end = loop {
                done[edge as usize] = true;
                chains.halfedges.push(h);
                let w = mesh.end(h);
                // The other feature edge here, the way on.
                let came = (edge, first[edge as usize] != h);
                let next = at[w as usize].iter().copied().find(|&end| end != came);
                match next {
                    Some(next) if through[w as usize] && !done[next.0 as usize] => {
                        edge = next.0;
                        h = from(next);
                    }
                    _ => break w,
                }
            };
            chains.push(faces, begin, end);
        }
        chains
    }

    /// Ends the polyline whose halfedges were pushed last: between
    /// `faces`, from mesh vertex `begin` to `end`.
    fn push(&mut self, faces: [u32; 2], begin: u32, end: u32) {
        // Within `MAX_EDGE_POINTS`, checked by the caller before it
        // samples.
        self.ends.push(self.halfedges.len() as u32);
        self.faces.push(faces);
        let corners = [begin, end].map(|v| {
            if self.corner_of[v as usize] == u32::MAX {
                self.corner_of[v as usize] = self.corners.len() as u32;
                self.corners.push(v);
            }
            self.corner_of[v as usize]
        });
        self.edge_corners.push(corners);
    }
}

/// How many triangles each patch of `mesh`, which must pass
/// [`Mesh::check`], is drawn as within `display`: [`tessellate`] draws
/// them face by face and [`weld`] patch by patch.
#[cfg(test)]
pub(crate) fn patch_triangles(mesh: &Mesh, display: &Display) -> Vec<u64> {
    Plan::new(mesh, display, &Limits::RENDER)
        .unwrap()
        .map_or(Vec::new(), |plan| {
            plan.levels.iter().map(Level::triangles).collect()
        })
}

/// A welded tessellation's origin, positions about it and triangles,
/// unchecked.
pub(crate) type Welded = ([f64; 3], Vec<[f64; 3]>, Vec<[u32; 3]>);

/// `mesh`, which must pass [`Mesh::check`], as an indexed triangle mesh
/// for export, with the samples and triangles of [`tessellate`] (the
/// strips choosing their diagonals from the `f64` points) but each point
/// once: a mesh vertex is one vertex whatever its normals, an edge's
/// inner samples are numbered once for both sides, and the triangles name
/// them by those numbers, so the welding follows the patch mesh's own
/// edges and vertices, never a distance. The origin is the middle of the
/// mesh vertices' box rounded to whole numbers, and each position is the
/// `f64` sample less the origin, rounded to the nearest `f32`: what
/// readers keep, so the check sees what they will (two samples that round
/// together are refused, not written).
/// Unchecked: [`ManifoldMesh::new`] checks it. Fails with
/// [`ManifoldError::TooLarge`] past `limits` (within
/// [`ManifoldMesh::MAX_VERTICES`] and [`ManifoldMesh::MAX_TRIANGLES`]),
/// and with [`ManifoldError::Empty`] for the empty mesh.
pub(crate) fn weld(
    mesh: &Mesh,
    display: &Display,
    limits: &Limits,
) -> Result<Welded, ManifoldError> {
    let plan = Plan::new(mesh, display, limits)
        .map_err(|_| ManifoldError::TooLarge)?
        .ok_or(ManifoldError::Empty)?;
    if !plan.fits(limits) {
        return Err(ManifoldError::TooLarge);
    }
    let Plan {
        first,
        curves,
        counts,
        tri_ids,
        edge_ids,
        inner,
        ..
    } = &plan;

    // A checked mesh that isn't empty has vertices.
    let bounds = Bounds3::around(mesh.verts()).ok_or(ManifoldError::Empty)?;
    let origin = ((bounds.min + bounds.max) * 0.5).round();
    let about = |p: DVec3| (p - origin).as_vec3().as_dvec3().to_array();

    // Mesh vertices first, by their own numbers (a checked mesh uses every
    // one), then each edge's inner samples along its first halfedge, then
    // each patch's inner points.
    let mut positions: Vec<[f64; 3]> = mesh.verts().iter().map(|&v| about(v)).collect();
    // The `f64` points of the vertices on edges, which the strips choose
    // their diagonals from, as drawing does.
    let mut exact: Vec<DVec3> = mesh.verts().to_vec();
    let edge_points: Vec<Vec<DVec3>> = par_map(edge_ids, |&e| {
        let n = counts[e as usize];
        let curve = &curves[e as usize];
        (1..n)
            .map(|s| curve.eval(f64::from(s) / f64::from(n)))
            .collect()
    });
    let mut edge_base = Vec::with_capacity(edge_points.len());
    for points in &edge_points {
        edge_base.push(positions.len() as u64);
        positions.extend(points.iter().map(|&p| about(p)));
        exact.extend_from_slice(points);
    }
    // Within the limits, checked above.
    let inner_base = positions.len() as u64;
    let sample_vertex = |h: u32, r: u32| -> u32 {
        let n = plan.n_of(h);
        if r == 0 {
            return mesh.halfedge(h).start;
        }
        if r == n {
            return mesh.end(h);
        }
        let e = mesh.halfedge(h).edge as usize;
        let s = if first[e] == h { r } else { n - r };
        (edge_base[e] + u64::from(s - 1)) as u32
    };

    let patches: Vec<(Vec<DVec3>, Vec<u32>)> = par_map(tri_ids, |&t| {
        let patch = mesh.patch(t as usize);
        let points: Vec<DVec3> = (plan.inner_params(t).into_iter())
            .map(|u| patch.eval(u))
            .collect();
        let base = (inner_base + inner[t as usize]) as u32;
        let indices = plan.patch_triangles(&patch, t, base, &points, |h, r| {
            let v = sample_vertex(h, r);
            (v, exact[v as usize])
        });
        (points, indices)
    });
    drop(exact);
    let mut triangles = Vec::with_capacity(plan.triangles as usize);
    for (points, indices) in patches {
        positions.extend(points.iter().map(|&p| about(p)));
        triangles.extend_from_slice(indices.as_chunks::<3>().0);
    }
    debug_assert_eq!(triangles.len() as u64, plan.triangles);
    debug_assert_eq!(positions.len() as u64, plan.least_vertices());
    Ok((origin.to_array(), positions, triangles))
}

/// A render vertex: its position and normal.
type Vertex = ([f32; 3], [f32; 3]);

/// The unit normal of `patch` at `u`. A checked patch's normal doesn't
/// vanish; should rounding make it, its fold direction stands in.
fn unit_normal(patch: &Patch, u: DVec3) -> DVec3 {
    patch
        .normal(u)
        .try_normalize()
        .or_else(|| patch.fold_direction())
        .unwrap_or(DVec3::Z)
}

/// How many equal parameter steps `curve` needs: chords within `chord` of
/// the curve at their middles, and the tangent turning at most
/// [`Display::MAX_TURN_DEGREES`] along each, up to
/// [`Display::MAX_SEGMENTS`]. Uses only correctly rounded arithmetic, so
/// every platform gets the same count.
pub(crate) fn segments(curve: &Conic3, chord: f64) -> u32 {
    let max = Display::MAX_SEGMENTS;
    // The whole turn is the angle between the end tangents; the chord
    // between their unit vectors is shorter, so this underestimates.
    let d0 = (curve.c - curve.p0).normalize_or_zero();
    let d1 = (curve.p1 - curve.c).normalize_or_zero();
    let mut n = steps((d0 - d1).length() / TURN);
    loop {
        let (error, turned) = measure(curve, n);
        if (error <= chord && !turned) || n >= max {
            return n;
        }
        // The chord error of a smooth curve falls with the square of the
        // step.
        let grow = if error > chord {
            steps(f64::from(n) * (error / chord).sqrt())
        } else {
            1
        };
        n = grow.max(n + 1).min(max);
    }
}

/// `x` rounded up to a segment count within `1..=MAX_SEGMENTS` (NaN to
/// the most).
fn steps(x: f64) -> u32 {
    let max = Display::MAX_SEGMENTS;
    if x.is_nan() {
        max
    } else {
        // Clamped first, so the cast is exact.
        x.ceil().clamp(1.0, f64::from(max)) as u32
    }
}

/// The largest distance of `curve`'s points halfway along each of `n`
/// equal steps from that step's chord, and whether its tangent turns by
/// more than [`Display::MAX_TURN_DEGREES`] along any step.
fn measure(curve: &Conic3, n: u32) -> (f64, bool) {
    let at = |s: u32| f64::from(s) / f64::from(n);
    let mut error = 0.0f64;
    let mut turned = false;
    let mut prev = curve.eval_deriv(0.0);
    for s in 0..n {
        let next = curve.eval_deriv(at(s + 1));
        let mid = curve.eval((f64::from(s) + 0.5) / f64::from(n));
        let d = distance_to_line(mid, prev.0, next.0);
        // NaN counts as too far.
        error = if d.is_nan() {
            f64::INFINITY
        } else {
            error.max(d)
        };
        let (a, b) = (prev.1.normalize_or_zero(), next.1.normalize_or_zero());
        if a != DVec3::ZERO && b != DVec3::ZERO && a.dot(b) < COS_TURN {
            turned = true;
        }
        prev = next;
    }
    (error, turned)
}

/// The distance from `x` to the line through `a` and `b` (to `a` if they
/// are the same point).
fn distance_to_line(x: DVec3, a: DVec3, b: DVec3) -> f64 {
    let (ab, ax) = (b - a, x - a);
    let len2 = ab.length_squared();
    if len2 > 0.0 {
        (ax - ab * (ax.dot(ab) / len2)).length()
    } else {
        ax.length()
    }
}

/// The most steps a patch's inner grid is refined to
/// ([`Plan::refine`]).
const MAX_INNER_STEPS: u32 = 4 * Display::MAX_SEGMENTS;

/// The rounds of [`Plan::refine`] that refine by as little as one step.
const FINE_ROUNDS: u32 = 4;

/// The steps the next round of [`Plan::refine`] gives a patch whose grid
/// of `m` steps is `error` from it, or `None` if that's within `chord`
/// (NaN counts as within: a finer grid wouldn't mend it) or the grid has
/// [`MAX_INNER_STEPS`] already.
fn finer(m: u32, error: f64, chord: f64, round: u32) -> Option<u32> {
    if error.is_nan() || error <= chord || m >= MAX_INNER_STEPS {
        return None;
    }
    // The error inside a smooth patch falls with the square of the step.
    // Where the ring's triangles set it, it falls slower (their edge sides
    // stay as they are), so this undershoots near the chord: one step more
    // at a time reaches the smallest grid within it, and a quarter more
    // once that has taken a few rounds bounds them.
    let wanted = (f64::from(m) * (error / chord).sqrt()).ceil();
    let at_least = if round < FINE_ROUNDS {
        m + 1
    } else {
        m + m.div_ceil(4)
    };
    Some(if wanted < f64::from(MAX_INNER_STEPS) {
        // Below the most, so the cast is exact.
        (wanted as u32).max(at_least)
    } else {
        MAX_INNER_STEPS
    })
}

/// Whether the patches of a face of `form` may be curved both ways, so
/// that their inner grids are measured and refined ([`Plan::refine`]):
/// spheres, tori, revolved conics, ellipsoids and faces of no known form.
/// A plane, cylinder or cone, circular or not, is straight along its
/// rulings, and its grid is within the chord where the edges are (but at
/// the corners of skewed patches; see `agents/kernel.md`). A quadric form
/// is a scaled cone or sphere: the cone written about its apex, with no
/// linear or constant term (a scale keeps both at 0), the ellipsoid with
/// its constant below 0.
fn curved_both_ways(form: &Form) -> bool {
    match form {
        Form::Plane { .. }
        | Form::Cylinder { .. }
        | Form::ConicCylinder { .. }
        | Form::Cone { .. } => false,
        Form::Quadric(q) => q.c != 0.0 || q.b != DVec3::ZERO,
        Form::Unknown | Form::Sphere { .. } | Form::Torus { .. } | Form::Revolved { .. } => true,
    }
}

/// The farthest the triangles `level` makes of `patch` get from it inside,
/// in `f64`, by the patch at the middle of each triangle (in parameters)
/// and at the middle of each side that isn't a segment of an edge (the
/// edges' own segments are [`segments`]'), each from the triangle's point
/// at the same mix of its corners, along the patch's normal there: the
/// triangles drawn and welded, as their strips choose from the same `f64`
/// points. A single triangle is measured too.
fn level_error(patch: &Patch, level: &Level) -> f64 {
    let (params, first, base) = level.params();
    let points: Vec<DVec3> = params.iter().map(|&u| patch.eval(u)).collect();
    let indices = level.triangulate(patch, base, &points[base as usize..], |i, r| {
        let v = first[i as usize] + r;
        (v, points[v as usize])
    });
    // Consecutive samples of one edge.
    let along_edge = |a: u32, b: u32| {
        a < base && b < base && a.abs_diff(b) == 1 && {
            let side = |v: u32| first.iter().rposition(|&f| f <= v);
            side(a) == side(b)
        }
    };
    // Along the normal: the patch also drifts along itself where its
    // parameters run unevenly, which is no error, and against the
    // triangle's plane a triangle steep to the patch (a ring corner on a
    // patch curved hard near it) reads near when it is far.
    let off = |u: DVec3, x: DVec3| (patch.eval(u) - x).dot(unit_normal(patch, u)).abs();
    let mut error = 0.0f64;
    for tri in indices.as_chunks::<3>().0 {
        let [pa, pb, pc] = tri.map(|v| points[v as usize]);
        let [ua, ub, uc] = tri.map(|v| params[v as usize]);
        error = error.max(off((ua + ub + uc) / 3.0, (pa + pb + pc) / 3.0));
        for i in 0..3 {
            let (a, b) = (tri[i], tri[(i + 1) % 3]);
            if !along_edge(a, b) {
                let (ua, ub) = (params[a as usize], params[b as usize]);
                let (pa, pb) = (points[a as usize], points[b as usize]);
                error = error.max(off((ua + ub) * 0.5, (pa + pb) * 0.5));
            }
        }
    }
    error
}

/// How a patch is sampled, from its three edges' segment counts: as one
/// triangle when every edge is one segment, and otherwise on the regular
/// grid of `m` steps, `max(counts, 3)` or more ([`Plan::refine`]), keeping
/// only the points at least a step in from the boundary. Those are the
/// inner grid of `l = m − 3` steps, at barycentric `(i + 1, j + 1, k + 1)
/// / m` with `i + j + k = l`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Level {
    counts: [u32; 3],
    /// `l`, or `None` for a single triangle.
    inner: Option<u32>,
}

impl Level {
    fn new(counts: [u32; 3]) -> Level {
        let most = counts.into_iter().max().unwrap_or(1);
        Level {
            counts,
            inner: (most > 1).then(|| most.max(3) - 3),
        }
    }

    /// The level of `counts` on a grid of `steps` (3 at least), or of
    /// [`Level::new`]'s if that's more; a single triangle stays one only
    /// for 1 step.
    fn with_steps(counts: [u32; 3], steps: u32) -> Level {
        let level = Level::new(counts);
        let wanted = (steps > 1).then(|| steps.max(3) - 3);
        Level {
            counts,
            inner: level.inner.max(wanted),
        }
    }

    /// The grid's steps `m` (1 for a single triangle).
    fn steps(&self) -> u32 {
        self.inner.map_or(1, |l| l + 3)
    }

    /// The barycentric parameters of all the samples: each side's, from
    /// corner `i` to corner `i + 1` (so the corners twice), then the inner
    /// points ([`Level::inner_params`]); with where each side's start and
    /// where the inner points start.
    fn params(&self) -> (Vec<DVec3>, [u32; 3], u32) {
        let mut params = Vec::new();
        let mut first = [0u32; 3];
        for (i, &n) in self.counts.iter().enumerate() {
            first[i] = params.len() as u32;
            for r in 0..=n {
                let s = f64::from(r) / f64::from(n);
                let mut u = DVec3::ZERO;
                u[i] = 1.0 - s;
                u[(i + 1) % 3] = s;
                params.push(u);
            }
        }
        let base = params.len() as u32;
        params.extend(self.inner_params());
        (params, first, base)
    }

    /// The barycentric parameters of the inner points, in the order
    /// [`Level::index`] numbers them, or none for a single triangle.
    fn inner_params(&self) -> Vec<DVec3> {
        let Some(l) = self.inner else {
            return Vec::new();
        };
        let m = f64::from(l + 3);
        let mut params = Vec::new();
        for k in 0..=l {
            for j in 0..=l - k {
                let i = l - j - k;
                params.push(DVec3::new(f64::from(i + 1), f64::from(j + 1), f64::from(k + 1)) / m);
            }
        }
        params
    }

    /// The triangles, as vertex ids: the inner points are `base` on, at
    /// `inner` (as [`Level::inner_params`] orders them), and `outer(i, r)`
    /// is the id and position of side `i`'s sample `r` (from corner `i`
    /// towards corner `i + 1`, `0..=counts[i]`). The ring's diagonals are
    /// chosen against `patch`, the patch they sample.
    fn triangulate(
        &self,
        patch: &Patch,
        base: u32,
        inner: &[DVec3],
        outer: impl Fn(u32, u32) -> (u32, DVec3),
    ) -> Vec<u32> {
        let Some(l) = self.inner else {
            return [0, 1, 2].map(|i| outer(i, 0).0).to_vec();
        };
        let m = f64::from(l + 3);
        let idx = |j: u32, k: u32| base + Level::index(l, j, k);
        let inner_at = |j: u32, k: u32| {
            let i = Level::index(l, j, k);
            let u = DVec3::new(f64::from(l - j - k + 1), f64::from(j + 1), f64::from(k + 1)) / m;
            (base + i, inner[i as usize], u)
        };
        let mut indices = Vec::new();
        for k in 0..l {
            for j in 0..l - k {
                indices.extend([idx(j, k), idx(j + 1, k), idx(j, k + 1)]);
                if j + k + 2 <= l {
                    indices.extend([idx(j + 1, k), idx(j + 1, k + 1), idx(j, k + 1)]);
                }
            }
        }
        let sides: [Vec<Sample>; 3] = [
            (0..=l).map(|s| inner_at(s, 0)).collect(),
            (0..=l).map(|s| inner_at(l - s, s)).collect(),
            (0..=l).map(|s| inner_at(0, l - s)).collect(),
        ];
        // Side `i`'s sample `r`.
        let edge_at = |i: usize, r: u32| {
            let (v, x) = outer(i as u32, r);
            let s = f64::from(r) / f64::from(self.counts[i]);
            let mut u = DVec3::ZERO;
            u[i] = 1.0 - s;
            u[(i + 1) % 3] = s;
            (v, x, u)
        };
        let off = |u: DVec3, x: DVec3| (patch.eval(u) - x).dot(unit_normal(patch, u)).abs();
        // Where each strip's triangles start and where its last one is.
        let mut strips = [(0, 0); 3];
        for (i, inner) in sides.iter().enumerate() {
            let outer: Vec<Sample> = (0..=self.counts[i]).map(|r| edge_at(i, r)).collect();
            let first = indices.len();
            stitch(&mut indices, &outer, inner, off);
            strips[i] = (first, indices.len() - 3);
        }
        // The diagonal from each corner to the inner grid's corner next to
        // it, flipped to join the edges' samples either side when that
        // keeps closer to the patch: where the grid runs skewed to the
        // edges (a cylinder triangle whose far corner is round the arc),
        // the inner grid's corner is two steps round from the patch's.
        // Only where both strips end in an edge segment there, and the
        // quad is convex in parameters, so the new triangles keep the
        // patch's orientation.
        let orient = |p: &Sample, q: &Sample, r: &Sample| p.2.dot(q.2.cross(r.2));
        for i in 0..3 {
            let before = (i + 2) % 3;
            let (a, b) = (strips[before].1, strips[i].0);
            let prev = edge_at(before, self.counts[before] - 1);
            let (corner, next, g) = (edge_at(i, 0), edge_at(i, 1), sides[i][0]);
            let turn = orient(&prev, &corner, &g);
            if indices[a..a + 3] == [prev.0, corner.0, g.0]
                && indices[b..b + 3] == [corner.0, next.0, g.0]
                && prefer(diagonal(&prev, &next, off), diagonal(&corner, &g, off))
                && orient(&prev, &corner, &next) * turn > 0.0
                && orient(&prev, &next, &g) * turn > 0.0
            {
                indices[a..a + 3].copy_from_slice(&[prev.0, corner.0, next.0]);
                indices[b..b + 3].copy_from_slice(&[prev.0, next.0, g.0]);
            }
        }
        indices
    }

    /// Points of the inner grid.
    fn inner_points(&self) -> u64 {
        self.inner.map_or(0, |l| {
            let l = u64::from(l);
            (l + 1) * (l + 2) / 2
        })
    }

    /// Triangles: the inner grid's `l²`, and a strip between each edge's
    /// `n` segments and the inner grid's side of `l`.
    fn triangles(&self) -> u64 {
        self.inner.map_or(1, |l| {
            let l = u64::from(l);
            l * l + 3 * l + self.counts.iter().map(|&n| u64::from(n)).sum::<u64>()
        })
    }

    /// The index of inner point `(j, k)` (`i = l − j − k`), row by row
    /// along `k`.
    fn index(l: u32, j: u32, k: u32) -> u32 {
        k * (l + 1) - k * k.saturating_sub(1) / 2 + j
    }
}

/// A sample of a patch: its vertex id, position and parameters.
type Sample = (u32, DVec3, DVec3);

/// How far the diagonal between `p` and `q` is from the patch at its
/// middle (`off`, from the parameters and position there), and its
/// squared length.
fn diagonal(p: &Sample, q: &Sample, off: impl Fn(DVec3, DVec3) -> f64) -> (f64, f64) {
    let error = off((p.2 + q.2) * 0.5, (p.1 + q.1) * 0.5);
    (error, p.1.distance_squared(q.1))
}

/// Whether the diagonal measured `x` ([`diagonal`]) is to be taken over
/// `y`: the one closer to the patch, and where the two are as close
/// (within a millionth of the longer), the shorter, `x` on a tie.
fn prefer((ex, lx): (f64, f64), (ey, ly): (f64, f64)) -> bool {
    if (ex - ey).abs() > 1e-6 * lx.max(ly).sqrt() {
        ex < ey
    } else {
        lx <= ly
    }
}

/// Appends the triangles of the strip between the polylines `outer` (an
/// edge's points) and `inner` (the inner grid's side next to it), which
/// run the same way with the patch to the left, as vertex ids. Each step
/// advances the side whose new diagonal [`prefer`] takes, the outer one
/// on a tie. A patch's grid can run skewed to its edges (a cylinder
/// triangle whose far corner is round the arc), and pairing points by
/// their parameters alone then makes triangles steps wide; so does
/// pairing them by length alone on a tall wall, where the heights of the
/// inner points differ by more than a step round.
fn stitch(
    out: &mut Vec<u32>,
    outer: &[Sample],
    inner: &[Sample],
    off: impl Fn(DVec3, DVec3) -> f64,
) {
    let (a, b) = (outer.len() - 1, inner.len() - 1);
    let (mut s, mut t) = (0, 0);
    while s < a || t < b {
        let advance_outer = t == b
            || (s < a
                && prefer(
                    diagonal(&outer[s + 1], &inner[t], &off),
                    diagonal(&outer[s], &inner[t + 1], &off),
                ));
        if advance_outer {
            out.extend([outer[s].0, outer[s + 1].0, inner[t].0]);
            s += 1;
        } else {
            out.extend([outer[s].0, inner[t + 1].0, inner[t].0]);
            t += 1;
        }
    }
}

#[cfg(test)]
mod tests;

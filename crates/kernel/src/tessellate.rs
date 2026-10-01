//! Tessellating a checked patch mesh into a [`RenderMesh`].
//!
//! Every [`Edge`](crate::mesh::Edge) record gets a number of segments from
//! its own curve alone, and its sample points are worked out once, along
//! the curve's first halfedge, and read by both patches beside it, so
//! neighbours share their boundary points to the bit and the mesh has no
//! cracks. Each patch is sampled on a regular grid inside, one step in
//! from its boundary, and strips of triangles join that grid to the
//! points of its three edges. Normals are the patches' own, shared across
//! an edge where the two sides agree within [`Display::SMOOTH_DEGREES`]
//! and split where they don't; splits and the boundaries between faces of
//! different keys ([`FaceName::key`](crate::mesh::FaceName::key)) are the
//! feature edges drawn with the mesh.
//!
//! The rules and reasons are written down in `agents/kernel.md`.

use glam::{DVec3, Vec3};

use crate::mesh::Mesh;
use crate::par::par_map;
use crate::patch::{Bounds3, Conic3, Patch};
use crate::{MeshError, RenderMesh, Tolerance};

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
    pub(crate) edges: u64,
}

impl Limits {
    /// [`RenderMesh::MAX_VERTICES`] and the others.
    pub(crate) const RENDER: Limits = Limits {
        vertices: RenderMesh::MAX_VERTICES as u64,
        indices: RenderMesh::MAX_INDICES as u64,
        edges: RenderMesh::MAX_EDGES as u64,
    };
}

/// `mesh`, which must pass [`Mesh::check`], as triangles: see the module
/// docs.
pub(crate) fn tessellate(mesh: &Mesh, display: &Display) -> Result<RenderMesh, MeshError> {
    tessellate_within(mesh, display, &Limits::RENDER)
}

/// [`tessellate`], failing with [`MeshError::TooLarge`] if the mesh
/// would have more of a part than `limits` allow, which must be within
/// [`Limits::RENDER`]. Each part is counted before it's made.
pub(crate) fn tessellate_within(
    mesh: &Mesh,
    display: &Display,
    limits: &Limits,
) -> Result<RenderMesh, MeshError> {
    let Some(bounds) = Bounds3::around(mesh.verts()) else {
        return Ok(RenderMesh::default());
    };
    if mesh.is_empty() {
        return Ok(RenderMesh::default());
    }
    let chord = display.chord((bounds.max - bounds.min).length());
    let halfedges = u32::try_from(mesh.tris().len() * 3).map_err(|_| MeshError::TooLarge)?;

    // Each edge along its first halfedge, the canonical direction its
    // samples run in. A checked mesh uses every edge twice.
    let mut first = vec![u32::MAX; mesh.edges().len()];
    for h in 0..halfedges {
        let e = mesh.halfedge(h).edge as usize;
        if first[e] == u32::MAX {
            first[e] = h;
        }
    }
    let curves: Vec<Conic3> = first.iter().map(|&h| curve(mesh, h)).collect();
    let counts: Vec<u32> = par_map(&curves, |c| segments(c, chord));
    let n_of = |h: u32| counts[mesh.halfedge(h).edge as usize];

    // What each patch makes, and whether it all fits, before sampling.
    let tri_ids: Vec<u32> = (0..halfedges / 3).collect();
    let levels: Vec<Level> = tri_ids
        .iter()
        .map(|&t| Level::new([0, 1, 2].map(|i| n_of(3 * t + i))))
        .collect();
    let mut triangles = 0u64;
    let mut inner = Vec::with_capacity(levels.len());
    let mut inner_total = 0u64;
    for level in &levels {
        triangles = triangles.saturating_add(level.triangles());
        inner.push(inner_total);
        inner_total = inner_total.saturating_add(level.inner_points());
    }
    let edge_points: u64 = counts.iter().map(|&n| u64::from(n - 1)).sum();
    let at_least = (mesh.verts().len() as u64)
        .saturating_add(edge_points)
        .saturating_add(inner_total);
    if triangles.saturating_mul(3) > limits.indices || at_least > limits.vertices {
        return Err(MeshError::TooLarge);
    }

    // Unit normals along each halfedge of each patch, at its edge's
    // sample points in the halfedge's own direction.
    let boundary: Vec<Vec<DVec3>> = par_map(&tri_ids, |&t| {
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

    let edge_ids: Vec<u32> = (0..mesh.edges().len() as u32).collect();
    let smooth: Vec<bool> = par_map(&edge_ids, |&e| {
        let (a, n) = (first[e as usize], counts[e as usize]);
        let b = mesh.halfedge(a).pair;
        (0..=n).all(|s| normal_at(a, s).dot(normal_at(b, n - s)) >= COS_SMOOTH)
    });
    let feature: Vec<bool> = edge_ids
        .iter()
        .map(|&e| {
            let a = first[e as usize];
            let b = mesh.halfedge(a).pair;
            let key = |h: u32| {
                mesh.faces()[mesh.tris()[h as usize / 3].face as usize]
                    .name
                    .key()
            };
            !smooth[e as usize] || key(a) != key(b)
        })
        .collect();
    let feature_segments: u64 = edge_ids
        .iter()
        .filter(|&&e| feature[e as usize])
        .map(|&e| u64::from(counts[e as usize]))
        .sum();
    if feature_segments > limits.edges {
        return Err(MeshError::TooLarge);
    }

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();

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
    let edge_vertices: Vec<Vec<Vertex>> = par_map(&edge_ids, |&e| {
        let (a, n) = (first[e as usize], counts[e as usize]);
        let b = mesh.halfedge(a).pair;
        let curve = &curves[e as usize];
        let points: Vec<[f32; 3]> = (1..n)
            .map(|s| curve.eval(f64::from(s) / f64::from(n)).as_vec3().to_array())
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
            positions.push(p);
            normals.push(n);
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

    let patches: Vec<(Vec<Vertex>, Vec<u32>)> = par_map(&tri_ids, |&t| {
        let level = levels[t as usize];
        let hs = [0, 1, 2].map(|i| 3 * t + i);
        let Some(l) = level.inner else {
            return (Vec::new(), hs.map(|h| corner_vertex[h as usize]).to_vec());
        };
        let patch = mesh.patch(t as usize);
        let base = (inner_base + inner[t as usize]) as u32;
        let m = f64::from(l + 3);
        let mut points = Vec::new();
        for k in 0..=l {
            for j in 0..=l - k {
                let i = l - j - k;
                let u = DVec3::new(f64::from(i + 1), f64::from(j + 1), f64::from(k + 1)) / m;
                let p = patch.eval(u);
                points.push((
                    p.as_vec3().to_array(),
                    unit_normal(&patch, u).as_vec3().to_array(),
                ));
            }
        }
        let idx = |j: u32, k: u32| base + Level::index(l, j, k);
        let inner_at = |j: u32, k: u32| {
            let i = Level::index(l, j, k);
            (base + i, Vec3::from(points[i as usize].0).as_dvec3())
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
        let sides: [Vec<(u32, DVec3)>; 3] = [
            (0..=l).map(|s| inner_at(s, 0)).collect(),
            (0..=l).map(|s| inner_at(l - s, s)).collect(),
            (0..=l).map(|s| inner_at(0, l - s)).collect(),
        ];
        for (h, inner) in hs.iter().zip(&sides) {
            let outer: Vec<(u32, DVec3)> = (0..=n_of(*h))
                .map(|r| {
                    let v = sample_vertex(*h, r);
                    (v, Vec3::from(positions[v as usize]).as_dvec3())
                })
                .collect();
            stitch(&mut indices, &outer, inner);
        }
        (points, indices)
    });
    let mut indices = Vec::new();
    for (points, tri_indices) in patches {
        for (p, n) in points {
            positions.push(p);
            normals.push(n);
        }
        indices.extend(tri_indices);
    }
    debug_assert_eq!(indices.len() as u64, 3 * triangles);
    debug_assert_eq!(positions.len() as u64, inner_base + inner_total);

    let mut edges = Vec::new();
    for &e in &edge_ids {
        if feature[e as usize] {
            let (h, n) = (first[e as usize], counts[e as usize]);
            edges.extend((0..n).map(|s| [sample_vertex(h, s), sample_vertex(h, s + 1)]));
        }
    }
    RenderMesh::from_parts(positions, normals, indices, edges)
}

/// A render vertex: its position and normal.
type Vertex = ([f32; 3], [f32; 3]);

/// The curve of halfedge `h`, from its start to its end.
fn curve(mesh: &Mesh, h: u32) -> Conic3 {
    let edge = mesh.edges()[mesh.halfedge(h).edge as usize];
    Conic3 {
        p0: mesh.verts()[mesh.halfedge(h).start as usize],
        c: edge.ctrl,
        w: edge.weight,
        p1: mesh.verts()[mesh.end(h) as usize],
    }
}

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

/// How a patch is sampled, from its three edges' segment counts: as one
/// triangle when every edge is one segment, and otherwise on the regular
/// grid of `m = max(counts, 3)` steps, keeping only the points at least a
/// step in from the boundary. Those are the inner grid of `l = m − 3`
/// steps, at barycentric `(i + 1, j + 1, k + 1) / m` with `i + j + k = l`.
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

/// Appends the triangles of the strip between the polylines `outer` (an
/// edge's points) and `inner` (the inner grid's side next to it), which
/// run the same way with the patch to the left, as vertex ids and
/// positions. Each step advances the side that makes the shorter new
/// diagonal, the outer one on a tie: a patch's grid can run skewed to its
/// edges (a cylinder triangle whose far corner is round the arc), and
/// pairing points by their parameters alone then makes triangles two
/// steps wide.
fn stitch(out: &mut Vec<u32>, outer: &[(u32, DVec3)], inner: &[(u32, DVec3)]) {
    let (a, b) = (outer.len() - 1, inner.len() - 1);
    let (mut s, mut t) = (0, 0);
    while s < a || t < b {
        let advance_outer = t == b
            || (s < a
                && outer[s + 1].1.distance_squared(inner[t].1)
                    <= outer[s].1.distance_squared(inner[t + 1].1));
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

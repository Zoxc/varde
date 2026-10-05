//! Where a plane meets a solid's curves and faces, for sketches' links:
//! the points where it crosses a chain of conics ([`crossings`]), the
//! curves where it cuts a face ([`face_section`]), and places along a
//! chain ([`sample`]), which a projection maps onto the plane.
//!
//! A plane is a point on it and its unit normal. A conic's signed
//! distance from it is a rational quadratic whose numerator, a quadratic
//! in the parameter, is solved for its roots: exact but for rounding. A
//! patch's is a quadratic form in its barycentric coordinates, whose zero
//! set is a conic in the patch's domain: it's found where it crosses the
//! patch's edges (the edge conics' roots, so two patches sharing an edge
//! find the same point but for rounding) and traced between them by
//! Newton's steps from the chord in the domain onto the zero set, each
//! place on the patch exactly. The pieces are joined end to end where
//! their ends are within the welding distance.

use std::collections::HashMap;

use glam::DVec3;

use crate::Solid;
use crate::patch::{Conic3, Patch};

/// A plane: a point on it and its unit normal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane {
    pub point: DVec3,
    pub normal: DVec3,
}

impl Plane {
    /// How far `p` is from it, on the side its normal points to
    /// positive.
    pub fn distance(&self, p: DVec3) -> f64 {
        self.normal.dot(p - self.point)
    }
}

/// Why a plane gives no section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionError {
    /// What it cuts lies in the plane, or a part of it does, within the
    /// welding distance: there's no crossing to find, but a whole curve
    /// or face.
    InPlane,
    /// More places than [`MAX_SECTION_PLACES`].
    TooComplex,
}

/// The most places a section or a sampling gives: well past what a face
/// cut across or an edge needs, few enough to fit a sketch's link.
pub const MAX_SECTION_PLACES: usize = 1 << 17;

/// How many places a piece of a section through one patch is traced at,
/// its ends included.
const TRACE: usize = 9;

/// One curve of a section: places along it, exactly on the face, in
/// order. A closed one runs from its last back to its first, which isn't
/// repeated.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub places: Vec<DVec3>,
    pub closed: bool,
}

/// The parameters in `[0, 1]` where `curve` crosses or touches `plane`,
/// increasing, or [`SectionError::InPlane`] if it lies in it: all its
/// control points within `within` of it.
fn conic_crossings(curve: &Conic3, plane: &Plane, within: f64) -> Result<Vec<f64>, SectionError> {
    let [p0, c, p1] = [curve.p0, curve.c, curve.p1].map(|p| plane.distance(p));
    if [p0, c, p1].iter().all(|d| d.abs() <= within) {
        return Err(SectionError::InPlane);
    }
    Ok(quadratic_roots([p0, c * curve.w, p1]))
}

/// The roots in `[0, 1]` of the quadratic in Bernstein form with
/// coefficients `b`: `b0 (1-t)² + 2 b1 t (1-t) + b2 t²`, increasing, a
/// double root once.
fn quadratic_roots([b0, b1, b2]: [f64; 3]) -> Vec<f64> {
    // In powers of t: a t² + b t + c.
    let a = b0 - 2.0 * b1 + b2;
    let b = 2.0 * (b1 - b0);
    let c = b0;
    let scale = b0.abs().max(b1.abs()).max(b2.abs());
    if !scale.is_finite() || scale <= 0.0 {
        return Vec::new();
    }
    let mut roots = Vec::new();
    if a.abs() <= 1e-14 * scale {
        if b.abs() > 1e-14 * scale {
            roots.push(-c / b);
        }
    } else {
        let disc = b * b - 4.0 * a * c;
        if disc >= 0.0 {
            let root = disc.sqrt();
            // Without cancellation: the larger root first, the other by
            // their product.
            let q = -0.5 * (b + b.signum() * root);
            if q != 0.0 {
                roots.push(q / a);
                roots.push(c / q);
            } else {
                roots.push(-b / (2.0 * a));
            }
        } else if disc > -1e-12 * b * b {
            // Touching, but for rounding.
            roots.push(-b / (2.0 * a));
        }
    }
    let mut roots: Vec<f64> = (roots.into_iter())
        .filter(|t| t.is_finite() && (-1e-12..=1.0 + 1e-12).contains(t))
        .map(|t| t.clamp(0.0, 1.0))
        .collect();
    roots.sort_by(f64::total_cmp);
    roots.dedup_by(|a, b| (*a - *b).abs() <= 1e-12);
    roots
}

/// The points where the chain of conics `curves` (end to end) crosses
/// `plane`, in the order along it, those within `within` of the one
/// before or the first once (the end of one conic and the start of the
/// next, or of a closed chain's last and its first). A conic lying in
/// the plane makes the chain [`SectionError::InPlane`].
pub fn crossings(
    curves: &[Conic3],
    plane: &Plane,
    within: f64,
) -> Result<Vec<DVec3>, SectionError> {
    let mut points: Vec<DVec3> = Vec::new();
    for curve in curves {
        for t in conic_crossings(curve, plane, within)? {
            let p = curve.eval(t);
            let near = |q: Option<&DVec3>| q.is_some_and(|q| q.distance(p) <= within);
            if !near(points.last()) && !near(points.first()) {
                points.push(p);
            }
            if points.len() > MAX_SECTION_PLACES {
                return Err(SectionError::TooComplex);
            }
        }
    }
    Ok(points)
}

/// Places along the chain of conics `curves` (end to end), `per` evenly
/// by parameter on each, and the chain's end unless `closed` (it's then
/// the start again): exactly on it.
pub fn sample(curves: &[Conic3], per: usize, closed: bool) -> Result<Vec<DVec3>, SectionError> {
    let per = per.max(1);
    let count = curves.len().saturating_mul(per).saturating_add(1);
    if count > MAX_SECTION_PLACES {
        return Err(SectionError::TooComplex);
    }
    let mut places = Vec::with_capacity(count);
    for curve in curves {
        places.extend((0..per).map(|k| curve.eval(k as f64 / per as f64)));
    }
    if !closed && let Some(last) = curves.last() {
        places.push(last.p1);
    }
    Ok(places)
}

/// Where `plane` cuts the face made of the triangles `tris` of `solid`:
/// its curves, each of places exactly on the face, joined where their
/// ends are within `weld`. [`SectionError::InPlane`] for a face lying in
/// the plane (every patch it meets within `weld` of it), and
/// [`SectionError::TooComplex`] past [`MAX_SECTION_PLACES`]. Empty where
/// the plane misses it.
pub fn face_section(
    solid: &Solid,
    tris: &[u32],
    plane: &Plane,
    weld: f64,
) -> Result<Vec<Section>, SectionError> {
    let mesh = solid.mesh();
    let mut pieces: Vec<Vec<DVec3>> = Vec::new();
    let mut places = 0usize;
    let mut in_plane = 0usize;
    for &tri in tris {
        let patch = mesh.patch(tri as usize);
        let form = Form::of(&patch, plane);
        if form.flat(weld) {
            in_plane += 1;
            continue;
        }
        if form.one_side() {
            continue;
        }
        for piece in form.pieces(&patch) {
            places = places.saturating_add(piece.len());
            if places > MAX_SECTION_PLACES {
                return Err(SectionError::TooComplex);
            }
            pieces.push(piece);
        }
    }
    if pieces.is_empty() && in_plane > 0 {
        return Err(SectionError::InPlane);
    }
    Ok(join(pieces, weld))
}

/// A patch's signed distance from a plane, as the quadratic form of its
/// numerator in barycentric coordinates: `N(u) = Σ a[i][j] u_i u_j`, the
/// corners on the diagonal and each edge's weight times its control
/// point's distance off it.
struct Form {
    a: [[f64; 3]; 3],
}

impl Form {
    fn of(patch: &Patch, plane: &Plane) -> Form {
        let mut a = [[0.0; 3]; 3];
        for i in 0..3 {
            let j = (i + 1) % 3;
            a[i][i] = plane.distance(patch.p[i]);
            let edge = plane.distance(patch.c[i]) * patch.w[i];
            a[i][j] = edge;
            a[j][i] = edge;
        }
        Form { a }
    }

    /// Whether the patch lies in the plane: every control point within
    /// `weld` of it (weights are about 1).
    fn flat(&self, weld: f64) -> bool {
        self.a.iter().flatten().all(|v| v.abs() <= weld)
    }

    /// Whether the patch is on one side of the plane, not touching it:
    /// its control points all strictly one side (its hull is).
    fn one_side(&self) -> bool {
        let values = self.a.iter().flatten();
        values.clone().all(|&v| v > 0.0) || values.clone().all(|&v| v < 0.0)
    }

    fn value(&self, u: DVec3) -> f64 {
        let u = u.to_array();
        let mut sum = 0.0;
        for i in 0..3 {
            for j in 0..3 {
                sum += self.a[i][j] * u[i] * u[j];
            }
        }
        sum
    }

    /// The gradient in the domain's plane, by `u0` and `u1` with `u2 =
    /// 1 - u0 - u1`.
    fn gradient(&self, u: DVec3) -> [f64; 2] {
        let u = u.to_array();
        let g: [f64; 3] =
            std::array::from_fn(|i| 2.0 * (0..3).map(|j| self.a[i][j] * u[j]).sum::<f64>());
        [g[0] - g[2], g[1] - g[2]]
    }

    /// `u` moved onto the zero set by Newton's steps along the gradient.
    fn settle(&self, mut u: DVec3) -> DVec3 {
        for _ in 0..12 {
            let value = self.value(u);
            let [gx, gy] = self.gradient(u);
            let length = gx * gx + gy * gy;
            if length.is_nan() || length <= 0.0 {
                break;
            }
            let (dx, dy) = (value * gx / length, value * gy / length);
            u = DVec3::new(u.x - dx, u.y - dy, 1.0 - (u.x - dx) - (u.y - dy));
            if dx.abs() + dy.abs() <= 1e-15 {
                break;
            }
        }
        u
    }

    /// Where the zero set crosses the patch's boundary: barycentric
    /// points, in order round it from corner 0, those at a corner once.
    fn boundary(&self) -> Vec<DVec3> {
        let mut found: Vec<DVec3> = Vec::new();
        for k in 0..3 {
            let next = (k + 1) % 3;
            let roots = quadratic_roots([self.a[k][k], self.a[k][next], self.a[next][next]]);
            for t in roots {
                let mut u = [0.0; 3];
                u[k] = 1.0 - t;
                u[next] = t;
                let u = DVec3::from_array(u);
                if !(found.iter()).any(|f| (*f - u).abs().max_element() <= 1e-12) {
                    found.push(u);
                }
            }
        }
        found
    }

    /// The pieces of the zero set across `patch`: each from one boundary
    /// crossing to another, as places on the patch. Crossings are paired
    /// in order round the boundary, starting at the first or the second,
    /// whichever keeps the pairs' chords nearer the zero set; an odd count
    /// (the plane only touching) gives none.
    fn pieces(&self, patch: &Patch) -> Vec<Vec<DVec3>> {
        let crossings = self.boundary();
        let n = crossings.len();
        if n < 2 || n % 2 == 1 {
            return Vec::new();
        }
        let pairs = |start: usize| -> Vec<(DVec3, DVec3)> {
            (0..n / 2)
                .map(|i| {
                    (
                        crossings[(start + 2 * i) % n],
                        crossings[(start + 2 * i + 1) % n],
                    )
                })
                .collect()
        };
        let off = |pairs: &[(DVec3, DVec3)]| -> f64 {
            (pairs.iter())
                .map(|&(a, b)| {
                    let mid = (a + b) * 0.5;
                    let [gx, gy] = self.gradient(mid);
                    self.value(mid).abs() / (gx * gx + gy * gy).sqrt().max(1e-300)
                })
                .sum()
        };
        let chosen = if n == 2 {
            pairs(0)
        } else {
            let (first, second) = (pairs(0), pairs(1));
            if off(&second) < off(&first) {
                second
            } else {
                first
            }
        };
        (chosen.into_iter())
            .map(|(a, b)| {
                (0..TRACE)
                    .map(|k| {
                        let along = k as f64 / (TRACE - 1) as f64;
                        let u = a * (1.0 - along) + b * along;
                        let u = if k == 0 || k == TRACE - 1 {
                            u
                        } else {
                            self.settle(u)
                        };
                        let u = u.clamp(DVec3::ZERO, DVec3::ONE);
                        patch.eval(u / (u.x + u.y + u.z))
                    })
                    .collect()
            })
            .collect()
    }
}

/// Whether `a` and `b` trace the same places, one way or the other: each
/// within `weld` of the other's in turn.
fn same_trace(a: &[DVec3], b: &[DVec3], weld: f64) -> bool {
    let near = |(p, q): (&DVec3, &DVec3)| p.distance(*q) <= weld;
    a.len() == b.len() && (a.iter().zip(b).all(near) || a.iter().zip(b.iter().rev()).all(near))
}

/// `pieces` joined end to end where their ends are within `weld`, each
/// joined curve open or, where it comes back to its start, closed. A
/// piece tracing another's places again is left out.
fn join(pieces: Vec<Vec<DVec3>>, weld: f64) -> Vec<Section> {
    let cell = weld.max(f64::MIN_POSITIVE) * 4.0;
    let key = |p: DVec3| {
        let q = (p / cell).floor();
        (q.x as i64, q.y as i64, q.z as i64)
    };
    // Each piece's ends, by grid cell: (piece, whether it's its end).
    let mut grid: HashMap<(i64, i64, i64), Vec<(usize, bool)>> = HashMap::new();
    for (i, piece) in pieces.iter().enumerate() {
        grid.entry(key(piece[0])).or_default().push((i, false));
        grid.entry(key(piece[piece.len() - 1]))
            .or_default()
            .push((i, true));
    }
    let mut used = vec![false; pieces.len()];
    // A piece along an edge lying in the plane is traced by the patch on
    // each side of it, the same places one way or the other: it's kept
    // once, or it would be joined to itself, running there and back.
    for (i, piece) in pieces.iter().enumerate() {
        if used[i] {
            continue;
        }
        let (x, y, z) = key(piece[0]);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    for &(j, _) in grid.get(&(x + dx, y + dy, z + dz)).into_iter().flatten() {
                        if j > i && !used[j] && same_trace(piece, &pieces[j], weld) {
                            used[j] = true;
                        }
                    }
                }
            }
        }
    }
    let near = |p: DVec3, used: &[bool]| -> Option<(usize, bool)> {
        let (x, y, z) = key(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    for &(i, end) in grid.get(&(x + dx, y + dy, z + dz)).into_iter().flatten() {
                        let at = if end {
                            pieces[i][pieces[i].len() - 1]
                        } else {
                            pieces[i][0]
                        };
                        if !used[i] && at.distance(p) <= weld {
                            return Some((i, end));
                        }
                    }
                }
            }
        }
        None
    };
    let mut sections = Vec::new();
    for start in 0..pieces.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut places = pieces[start].clone();
        // On from its end, then back from its start.
        for forward in [true, false] {
            loop {
                let at = if forward {
                    places[places.len() - 1]
                } else {
                    places[0]
                };
                let Some((next, end)) = near(at, &used) else {
                    break;
                };
                used[next] = true;
                let mut more = pieces[next].clone();
                // Joined at `end` of `next`: run it away from there.
                if forward == end {
                    more.reverse();
                }
                if forward {
                    places.extend_from_slice(&more[1..]);
                } else {
                    more.pop();
                    more.extend_from_slice(&places);
                    places = more;
                }
            }
        }
        let closed = places.len() > 2 && places[0].distance(places[places.len() - 1]) <= weld;
        if closed {
            places.pop();
        }
        sections.push(Section { places, closed });
    }
    sections
}

#[cfg(test)]
mod tests;

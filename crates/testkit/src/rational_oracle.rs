//! The exact oracle for the snap's fixtures: what two bodies near each
//! other are to each other, decided in exact rational arithmetic
//! (`malachite::Rational`) from the fixtures' plain data
//! ([`crate::sheets`]), for checking the Varde kernel's snap. It shares
//! no code with the kernel: every answer is derived here from the
//! vertex positions alone.
//!
//! # Flat bodies ([`ExactPair`])
//!
//! Every f64 is an exact dyadic rational, so a [`Polygons`] body is taken
//! exactly ([`ExactBody`]): each face's plane `n·x = c` with `n` its
//! Newell normal (twice its vector area, along its outward side), every
//! vertex checked on it exactly, and the face cut into triangles by ear
//! clipping in exact arithmetic (each counter-clockwise about `n`). On
//! them the oracle answers:
//! - [`ExactPair::face_pairs`]: per face of `a` and face of `b` within a
//!   distance, the squared distance exactly and how the faces meet
//!   ([`Kind`]): apart, coplanar (the overlap's convex pieces, co-oriented
//!   or opposed), or meeting off-plane (the intersection's segments, and
//!   whether each face passes through the other's plane);
//! - [`ExactPair::gap`]: the gap along a direction `ℓ` over the common
//!   projection of chosen faces of each: cells (convex polygons) on which
//!   both faces are graphs over the plane `ℓ⊥`, with the gap `s` at each
//!   corner (`x_b = x_a + s·ℓ`, so `s > 0` where `b` lies on the `+ℓ`
//!   side of `a`; affine over a cell, so its extremes are at corners)
//!   and where it is zero ([`GapCell::zero_set`]): the rim of a gap that
//!   changes sign, the coincident part of a flush pair;
//! - [`ExactPair::coincident_edges`]: the edges of `a` and `b` whose
//!   projections along `ℓ` overlap in a segment of positive length (the
//!   zero-area regions a snap along `ℓ` leaves), with the gap at its ends;
//! - [`ExactBody::winding`]: a closed body's winding number at a point,
//!   or that the point is on its boundary.
//!
//! Squared distances, gaps, cell corners and areas are exact; nothing is
//! compared against a tolerance. Costs grow with the number of face pairs:
//! boxes (exact) skip the pairs that can't meet the query.
//!
//! # The smooth pair ([`SmoothOracle`])
//!
//! A biquadratic sheet over a cylinder: the squared distance `D(u, v)`
//! from the sheet's point to the cylinder's axis is a polynomial of
//! degree 4 in each parameter with rational coefficients, so the side of
//! the cylinder a point is on (`D − r²`) is exact. The least gap `√D − r`
//! is bounded by branch and bound on `D`'s Bernstein form (exact) and
//! then a square root in big floats (`malachite::Float`) rounded outward;
//! the gap's zeros along a parameter line (where the sheet crosses the
//! cylinder) are isolated exactly.

mod smooth;
#[cfg(test)]
mod tests;

pub use smooth::{GapBounds, Least, Line, Root, SmoothOracle, SmoothRelation};

use crate::sheets::{Pair, Polygons};
use malachite::Rational;
use malachite::base::num::basic::traits::Zero;
use std::cmp::Ordering;

/// An exact point.
pub type P3 = [Rational; 3];
/// An exact point of a coordinate plane.
pub type P2 = [Rational; 2];

/// Which body of a pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Side {
    A,
    B,
}

/// Inputs the oracle can't take.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OracleError {
    /// A vertex coordinate is NaN or infinite.
    NotFinite { side: Side, vertex: u32 },
    /// A face with fewer than three corners, a corner index out of range,
    /// zero area, or a loop that isn't a simple polygon.
    BadFace { side: Side, face: u32 },
    /// A face's corner off the plane of the face.
    NotPlanar { side: Side, face: u32, vertex: u32 },
    /// A face asked for its gap along `ℓ` is parallel to `ℓ`.
    NotGraph { side: Side, face: u32 },
    /// A direction that is zero or not finite, or a bound that is
    /// negative or not finite.
    BadInput,
    /// A winding number asked of an open sheet.
    NotClosed { side: Side },
    /// No ray of the oracle's fixed directions from the point misses
    /// every edge and vertex of the body (see [`ExactBody::winding`]).
    NoGenericRay,
    /// The smooth pair's net has weights that aren't one positive value,
    /// a non-finite value, or a zero axis.
    BadNet,
}

/// A polygon body taken exactly (module docs).
#[derive(Clone, Debug)]
pub struct ExactBody {
    pub side: Side,
    pub closed: bool,
    pub verts: Vec<P3>,
    pub faces: Vec<ExactFace>,
}

/// A planar face taken exactly.
#[derive(Clone, Debug)]
pub struct ExactFace {
    /// Its loop of vertex indices, as given.
    pub corners: Vec<u32>,
    /// The Newell normal: twice the face's vector area.
    pub normal: P3,
    /// `normal·x` at every point of the face.
    pub offset: Rational,
    /// Its triangles (vertex indices), counter-clockwise about `normal`,
    /// covering the face exactly.
    pub triangles: Vec<[u32; 3]>,
    /// The corners' bounding box.
    pub lo: P3,
    pub hi: P3,
}

/// Two bodies taken exactly.
#[derive(Clone, Debug)]
pub struct ExactPair {
    pub a: ExactBody,
    pub b: ExactBody,
}

/// How a face of `a` and a face of `b` meet ([`ExactPair::face_pairs`]).
#[derive(Clone, Debug, PartialEq)]
pub struct FacePair {
    pub a: u32,
    pub b: u32,
    /// The squared distance between the closed faces.
    pub dist2: Rational,
    pub kind: Kind,
}

/// The kind of a [`FacePair`].
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// The faces have no common point (`dist2 > 0`).
    Apart,
    /// The faces lie in one plane and meet: `opposed` if their normals
    /// point opposite ways; `pieces`, the overlap's convex pieces of
    /// positive area (counter-clockwise about `a`'s normal), none where
    /// they meet only along their boundaries; `area2`, the overlap's
    /// area squared.
    Coplanar {
        opposed: bool,
        pieces: Vec<Vec<P3>>,
        area2: Rational,
    },
    /// The faces lie in two planes and meet: `segments`, the
    /// intersection's pieces of positive length along the planes' common
    /// line `n_a × n_b` (in that order along it, merged where they join;
    /// none where the faces meet at points); `a_crosses` if `a` has
    /// corners strictly on both sides of `b`'s plane, `b_crosses` alike.
    Meet {
        segments: Vec<[P3; 2]>,
        a_crosses: bool,
        b_crosses: bool,
    },
}

impl FacePair {
    /// The faces pass through each other along a segment: they meet
    /// off-plane in a segment of positive length, each face reaching both
    /// sides of the other's plane.
    pub fn is_transversal(&self) -> bool {
        matches!(&self.kind, Kind::Meet { segments, a_crosses: true, b_crosses: true } if !segments.is_empty())
    }
}

/// The gap along `ℓ` over chosen faces ([`ExactPair::gap`]).
#[derive(Clone, Debug, PartialEq)]
pub struct GapOverlay {
    /// The direction, exactly.
    pub ell: P3,
    /// The coordinate axis the cells are projected along `ℓ` onto the
    /// plane of (`ℓ`'s largest component, the first on a tie).
    pub axis: usize,
    /// In order of `a`'s faces as given, then `b`'s, then their
    /// triangles.
    pub cells: Vec<GapCell>,
}

/// A convex cell of a [`GapOverlay`]: the common projection of a
/// triangle of face `a` and one of face `b`.
#[derive(Clone, Debug, PartialEq)]
pub struct GapCell {
    pub a: u32,
    pub b: u32,
    /// Its corners on `b`'s face, counter-clockwise in the projection.
    pub on_b: Vec<P3>,
    /// The same corners' points on `a`'s face: `on_b[k] − s[k]·ℓ`.
    pub on_a: Vec<P3>,
    /// The gap at each corner.
    pub s: Vec<Rational>,
    /// Its area projected along `ℓ` onto the plane of the overlay's axis.
    pub area: Rational,
}

/// Where a cell's gap is zero ([`GapCell::zero_set`]).
#[derive(Clone, Debug, PartialEq)]
pub enum ZeroSet {
    None,
    /// At one point of `b`'s face (a corner).
    Point(P3),
    /// Along a segment of `b`'s face.
    Segment([P3; 2]),
    /// Everywhere: the faces coincide over the cell.
    Whole,
}

impl GapCell {
    /// The least gap over the cell.
    pub fn min_s(&self) -> &Rational {
        let mut m = &self.s[0];
        for x in &self.s {
            if x < m {
                m = x;
            }
        }
        m
    }

    /// The greatest gap over the cell.
    pub fn max_s(&self) -> &Rational {
        let mut m = &self.s[0];
        for x in &self.s {
            if x > m {
                m = x;
            }
        }
        m
    }

    /// Where the gap is zero, on `b`'s face.
    pub fn zero_set(&self) -> ZeroSet {
        let n = self.s.len();
        let signs: Vec<Ordering> = self.s.iter().map(|x| x.cmp(&Rational::ZERO)).collect();
        if signs.iter().all(|&o| o == Ordering::Equal) {
            return ZeroSet::Whole;
        }
        if !signs.contains(&Ordering::Less) && !signs.contains(&Ordering::Greater) {
            return ZeroSet::None;
        }
        let mut pts: Vec<P3> = Vec::new();
        for k in 0..n {
            let j = (k + 1) % n;
            if signs[k] == Ordering::Equal {
                push_new(&mut pts, self.on_b[k].clone());
            } else if signs[j] != Ordering::Equal && signs[k] != signs[j] {
                let t = &self.s[k] / (&self.s[k] - &self.s[j]);
                push_new(&mut pts, lerp3(&self.on_b[k], &self.on_b[j], &t));
            }
        }
        match pts.len() {
            0 => ZeroSet::None,
            1 => ZeroSet::Point(pts.remove(0)),
            _ => {
                // A line meets a convex polygon in at most a segment: the
                // two points farthest apart are its ends.
                let (mut bi, mut bj, mut best) = (0, 1, Rational::ZERO);
                for i in 0..pts.len() {
                    for j in i + 1..pts.len() {
                        let d = dist2(&pts[i], &pts[j]);
                        if d > best {
                            (bi, bj, best) = (i, j, d);
                        }
                    }
                }
                ZeroSet::Segment([pts[bi].clone(), pts[bj].clone()])
            }
        }
    }
}

impl GapOverlay {
    /// The least and greatest gap over all cells; `None` without cells.
    pub fn range(&self) -> Option<[Rational; 2]> {
        let mut out: Option<[Rational; 2]> = None;
        for c in &self.cells {
            let (lo, hi) = (c.min_s(), c.max_s());
            out = Some(match out {
                None => [lo.clone(), hi.clone()],
                Some([l, h]) => [
                    if lo < &l { lo.clone() } else { l },
                    if hi > &h { hi.clone() } else { h },
                ],
            });
        }
        out
    }

    /// The cells' total projected area.
    pub fn area(&self) -> Rational {
        let mut a = Rational::ZERO;
        for c in &self.cells {
            a += &c.area;
        }
        a
    }
}

/// An overlap of an edge of `a` and an edge of `b` seen along `ℓ`
/// ([`ExactPair::coincident_edges`]).
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeOverlap {
    /// The edges, as their vertex indices, low first.
    pub a: [u32; 2],
    pub b: [u32; 2],
    /// The overlap's ends on `a`'s edge, from its low vertex's side.
    pub on_a: [P3; 2],
    /// The same ends on `b`'s edge.
    pub on_b: [P3; 2],
    /// The gap at each end: `on_b[k] = on_a[k] + s[k]·ℓ`.
    pub s: [Rational; 2],
}

/// A closed body's winding number at a point ([`ExactBody::winding`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Winding {
    /// The point lies on the boundary.
    On,
    Number(i64),
}

/// The ray directions [`ExactBody::winding`] tries, in order: integer
/// vectors in no coordinate plane and no diagonal plane, so that a ray
/// from a point of a fixture's grid rarely runs through an edge.
const RAYS: [[i64; 3]; 8] = [
    [3, 5, 7],
    [-7, 11, 13],
    [11, -13, 5],
    [-5, -17, 19],
    [17, 3, -23],
    [-19, 23, 2],
    [29, -2, 31],
    [-31, -29, 37],
];

impl ExactPair {
    /// The pair taken exactly.
    pub fn new(p: &Pair) -> Result<ExactPair, OracleError> {
        Ok(ExactPair {
            a: ExactBody::new(&p.a, Side::A)?,
            b: ExactBody::new(&p.b, Side::B)?,
        })
    }

    /// The body on `side`.
    pub fn body(&self, side: Side) -> &ExactBody {
        match side {
            Side::A => &self.a,
            Side::B => &self.b,
        }
    }

    /// Every pair of a face of `a` and a face of `b` at distance at most
    /// `within` (mm, compared exactly: `dist2 ≤ within²`), in order of
    /// `a`'s face, then `b`'s.
    pub fn face_pairs(&self, within: f64) -> Result<Vec<FacePair>, OracleError> {
        if !within.is_finite() || within < 0.0 {
            return Err(OracleError::BadInput);
        }
        let w = q(within);
        let w2 = &w * &w;
        let mut out = Vec::new();
        for (ia, fa) in self.a.faces.iter().enumerate() {
            for (ib, fb) in self.b.faces.iter().enumerate() {
                if box_gap2(&fa.lo, &fa.hi, &fb.lo, &fb.hi) > w2 {
                    continue;
                }
                let d2 = face_dist2(&self.a, fa, &self.b, fb);
                if d2 > w2 {
                    continue;
                }
                let kind = if d2 > Rational::ZERO {
                    Kind::Apart
                } else {
                    meet(&self.a, fa, &self.b, fb)
                };
                out.push(FacePair {
                    a: ia as u32,
                    b: ib as u32,
                    dist2: d2,
                    kind,
                });
            }
        }
        Ok(out)
    }

    /// The gap along `ell` from the faces `a_faces` of `a` to the faces
    /// `b_faces` of `b` (module docs), each of them a graph over `ℓ⊥`
    /// (`n·ℓ ≠ 0`, else [`OracleError::NotGraph`]).
    pub fn gap(
        &self,
        a_faces: &[u32],
        b_faces: &[u32],
        ell: [f64; 3],
    ) -> Result<GapOverlay, OracleError> {
        let (l, axis) = direction(ell)?;
        let pa = Projected::new(&self.a, &l, axis);
        let pb = Projected::new(&self.b, &l, axis);
        let check = |body: &ExactBody, faces: &[u32]| -> Result<(), OracleError> {
            for &f in faces {
                let face = body.face(f)?;
                if dot3(&face.normal, &l) == 0 {
                    return Err(OracleError::NotGraph {
                        side: body.side,
                        face: f,
                    });
                }
            }
            Ok(())
        };
        check(&self.a, a_faces)?;
        check(&self.b, b_faces)?;
        let mut cells = Vec::new();
        for &fa in a_faces {
            let face_a = &self.a.faces[fa as usize];
            for &fb in b_faces {
                let face_b = &self.b.faces[fb as usize];
                for ta in &face_a.triangles {
                    let tri_a = pa.triangle(ta);
                    for tb in &face_b.triangles {
                        let tri_b = pb.triangle(tb);
                        if !boxes_meet2(&tri_a, &tri_b) {
                            continue;
                        }
                        let poly = clip_convex(&tri_b, &tri_a);
                        let area = area2d(&poly) / Rational::from(2);
                        if area <= 0 {
                            continue;
                        }
                        let on_b: Vec<P3> = poly
                            .iter()
                            .map(|y| lift(y, &face_b.normal, &face_b.offset, &l, axis))
                            .collect();
                        let on_a: Vec<P3> = poly
                            .iter()
                            .map(|y| lift(y, &face_a.normal, &face_a.offset, &l, axis))
                            .collect();
                        let s = on_b
                            .iter()
                            .zip(&on_a)
                            .map(|(xb, xa)| (&xb[axis] - &xa[axis]) / &l[axis])
                            .collect();
                        cells.push(GapCell {
                            a: fa,
                            b: fb,
                            on_b,
                            on_a,
                            s,
                            area,
                        });
                    }
                }
            }
        }
        Ok(GapOverlay {
            ell: l,
            axis,
            cells,
        })
    }

    /// Every edge of `a` and edge of `b` whose projections along `ell`
    /// lie on one line and overlap in a segment of positive length, in
    /// order of `a`'s edge, then `b`'s ([`ExactBody::edges`]). Edges
    /// along `ℓ` project to points and are left out.
    pub fn coincident_edges(&self, ell: [f64; 3]) -> Result<Vec<EdgeOverlap>, OracleError> {
        let (l, axis) = direction(ell)?;
        let pa = Projected::new(&self.a, &l, axis);
        let pb = Projected::new(&self.b, &l, axis);
        let eb = self.b.edges();
        let mut out = Vec::new();
        for ea in self.a.edges() {
            let (y0, y1) = (&pa.pts[ea[0] as usize], &pa.pts[ea[1] as usize]);
            let dir = sub2(y1, y0);
            let len2 = dot2(&dir, &dir);
            if len2 == 0 {
                continue;
            }
            for &e in &eb {
                let (z0, z1) = (&pb.pts[e[0] as usize], &pb.pts[e[1] as usize]);
                if orient2(y0, y1, z0) != 0 || orient2(y0, y1, z1) != 0 || z0 == z1 {
                    continue;
                }
                let t0 = dot2(&sub2(z0, y0), &dir) / &len2;
                let t1 = dot2(&sub2(z1, y0), &dir) / &len2;
                let (tl, th) = if t0 < t1 { (t0, t1) } else { (t1, t0) };
                let lo = if tl > 0 { tl } else { Rational::ZERO };
                let hi = if th < 1 { th } else { Rational::from(1) };
                if lo >= hi {
                    continue;
                }
                let (a0, a1) = (&self.a.verts[ea[0] as usize], &self.a.verts[ea[1] as usize]);
                let (b0, b1) = (&self.b.verts[e[0] as usize], &self.b.verts[e[1] as usize]);
                let zd = sub2(z1, z0);
                let zl = dot2(&zd, &zd);
                let ends = [lo, hi].map(|t| {
                    let on_a = lerp3(a0, a1, &t);
                    let y = [&y0[0] + &(&dir[0] * &t), &y0[1] + &(&dir[1] * &t)];
                    let u = dot2(&sub2(&y, z0), &zd) / &zl;
                    let on_b = lerp3(b0, b1, &u);
                    let s = (&on_b[axis] - &on_a[axis]) / &l[axis];
                    (on_a, on_b, s)
                });
                let [(a_lo, b_lo, s_lo), (a_hi, b_hi, s_hi)] = ends;
                out.push(EdgeOverlap {
                    a: ea,
                    b: e,
                    on_a: [a_lo, a_hi],
                    on_b: [b_lo, b_hi],
                    s: [s_lo, s_hi],
                });
            }
        }
        Ok(out)
    }
}

impl ExactBody {
    /// The body taken exactly; `side` names it in errors.
    pub fn new(p: &Polygons, side: Side) -> Result<ExactBody, OracleError> {
        let mut verts = Vec::with_capacity(p.verts.len());
        for (k, v) in p.verts.iter().enumerate() {
            if !v.iter().all(|x| x.is_finite()) {
                return Err(OracleError::NotFinite {
                    side,
                    vertex: k as u32,
                });
            }
            verts.push(v.map(q));
        }
        let mut faces = Vec::with_capacity(p.faces.len());
        for (f, corners) in p.faces.iter().enumerate() {
            faces.push(exact_face(&verts, corners, side, f as u32)?);
        }
        Ok(ExactBody {
            side,
            closed: p.closed,
            verts,
            faces,
        })
    }

    fn face(&self, f: u32) -> Result<&ExactFace, OracleError> {
        self.faces.get(f as usize).ok_or(OracleError::BadFace {
            side: self.side,
            face: f,
        })
    }

    /// The faces whose outward normal has a positive component along
    /// `ell`: the faces that are graphs over `ℓ⊥` facing `+ℓ`.
    pub fn facing(&self, ell: [f64; 3]) -> Result<Vec<u32>, OracleError> {
        let (l, _) = direction(ell)?;
        Ok((0..self.faces.len() as u32)
            .filter(|&f| dot3(&self.faces[f as usize].normal, &l) > 0)
            .collect())
    }

    /// The faces' undirected edges, as vertex indices low first, sorted,
    /// each once.
    pub fn edges(&self) -> Vec<[u32; 2]> {
        let mut out: Vec<[u32; 2]> = self
            .faces
            .iter()
            .flat_map(|f| {
                let c = &f.corners;
                (0..c.len()).map(move |k| {
                    let (x, y) = (c[k], c[(k + 1) % c.len()]);
                    if x < y { [x, y] } else { [y, x] }
                })
            })
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The winding number at `p` of a closed body (its faces facing out:
    /// 1 inside, 0 outside), or [`Winding::On`] on a face. Counted along
    /// the first ray of a fixed list of directions that meets no
    /// triangle's edge or vertex and lies in no triangle's plane, as the
    /// signed crossings with the triangles, each decided exactly.
    pub fn winding(&self, p: &P3) -> Result<Winding, OracleError> {
        if !self.closed {
            return Err(OracleError::NotClosed { side: self.side });
        }
        for f in &self.faces {
            for t in &f.triangles {
                let tri = self.tri(t);
                if dot3(&tri.n, p) == tri.c && tri.contains(p) != Inside::Out {
                    return Ok(Winding::On);
                }
            }
        }
        'ray: for r in RAYS {
            let dir = r.map(Rational::from);
            let mut w = 0i64;
            for f in &self.faces {
                for t in &f.triangles {
                    let tri = self.tri(t);
                    let nd = dot3(&tri.n, &dir);
                    let sp = &tri.c - dot3(&tri.n, p);
                    if nd == 0 {
                        if sp == 0 {
                            continue 'ray;
                        }
                        continue;
                    }
                    let s = &sp / &nd;
                    if s <= 0 {
                        continue;
                    }
                    let hit = add3(p, &scale3(&dir, &s));
                    match tri.contains(&hit) {
                        Inside::In => w += if nd > 0 { 1 } else { -1 },
                        Inside::Edge => continue 'ray,
                        Inside::Out => {}
                    }
                }
            }
            return Ok(Winding::Number(w));
        }
        Err(OracleError::NoGenericRay)
    }

    fn tri(&self, t: &[u32; 3]) -> Tri {
        Tri::new(t.map(|k| self.verts[k as usize].clone()))
    }
}

/// The face `corners` of `verts` taken exactly (module docs).
fn exact_face(
    verts: &[P3],
    corners: &[u32],
    side: Side,
    face: u32,
) -> Result<ExactFace, OracleError> {
    let bad = OracleError::BadFace { side, face };
    if corners.len() < 3 || corners.iter().any(|&k| k as usize >= verts.len()) {
        return Err(bad);
    }
    let pts: Vec<&P3> = corners.iter().map(|&k| &verts[k as usize]).collect();
    let mut normal = zero3();
    for k in 0..pts.len() {
        normal = add3(&normal, &cross3(pts[k], pts[(k + 1) % pts.len()]));
    }
    if normal.iter().all(|x| *x == 0) {
        return Err(bad);
    }
    let offset = dot3(&normal, pts[0]);
    for (k, p) in pts.iter().enumerate() {
        if dot3(&normal, p) != offset {
            return Err(OracleError::NotPlanar {
                side,
                face,
                vertex: corners[k],
            });
        }
    }
    let axis = largest(&normal);
    let flip = normal[axis] < 0;
    let flat: Vec<P2> = pts.iter().map(|p| drop_axis(p, axis)).collect();
    let triangles = ear_clip(&flat, flip)
        .ok_or(bad)?
        .into_iter()
        .map(|t| t.map(|k| corners[k]))
        .collect();
    let mut lo = pts[0].clone();
    let mut hi = pts[0].clone();
    for p in &pts {
        for c in 0..3 {
            if p[c] < lo[c] {
                lo[c] = p[c].clone();
            }
            if p[c] > hi[c] {
                hi[c] = p[c].clone();
            }
        }
    }
    Ok(ExactFace {
        corners: corners.to_vec(),
        normal,
        offset,
        triangles,
        lo,
        hi,
    })
}

/// Triangles (indices into `poly`) covering the simple polygon `poly`,
/// counter-clockwise (clockwise if `flip`, then each triangle too); `None`
/// if it isn't simple. Corners running straight on are dropped first.
fn ear_clip(poly: &[P2], flip: bool) -> Option<Vec<[usize; 3]>> {
    let turn = |a: &P2, b: &P2, c: &P2| {
        let o = orient2(a, b, c);
        if flip { -o } else { o }
    };
    let mut idx: Vec<usize> = (0..poly.len()).collect();
    // Drop straight-on corners (`b` strictly between its neighbours).
    let mut k = 0;
    while idx.len() > 3 && k < idx.len() {
        let n = idx.len();
        let (a, b, c) = (
            &poly[idx[(k + n - 1) % n]],
            &poly[idx[k]],
            &poly[idx[(k + 1) % n]],
        );
        if orient2(a, b, c) == 0 && dot2(&sub2(b, a), &sub2(c, b)) > 0 {
            idx.remove(k);
            k = 0;
        } else {
            k += 1;
        }
    }
    let mut out = Vec::new();
    while idx.len() > 3 {
        let n = idx.len();
        let ear = (0..n).find(|&k| {
            let (i, j, l) = (idx[(k + n - 1) % n], idx[k], idx[(k + 1) % n]);
            let (a, b, c) = (&poly[i], &poly[j], &poly[l]);
            if turn(a, b, c) <= 0 {
                return false;
            }
            idx.iter().all(|&m| {
                m == i
                    || m == j
                    || m == l
                    || !(turn(a, b, &poly[m]) >= 0
                        && turn(b, c, &poly[m]) >= 0
                        && turn(c, a, &poly[m]) >= 0)
            })
        })?;
        out.push([idx[(ear + n - 1) % n], idx[ear], idx[(ear + 1) % n]]);
        idx.remove(ear);
    }
    if turn(&poly[idx[0]], &poly[idx[1]], &poly[idx[2]]) <= 0 {
        return None;
    }
    out.push([idx[0], idx[1], idx[2]]);
    Some(out)
}

/// A triangle with its plane `n·x = c`, `n = (p1 − p0) × (p2 − p0)`.
struct Tri {
    p: [P3; 3],
    n: P3,
    c: Rational,
}

/// Where a point of a triangle's plane lies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Inside {
    In,
    Edge,
    Out,
}

impl Tri {
    fn new(p: [P3; 3]) -> Tri {
        let n = cross3(&sub3(&p[1], &p[0]), &sub3(&p[2], &p[0]));
        let c = dot3(&n, &p[0]);
        Tri { p, n, c }
    }

    /// Where `x`, a point of the plane, lies against the closed triangle.
    fn contains(&self, x: &P3) -> Inside {
        let mut edge = false;
        for k in 0..3 {
            let (a, b) = (&self.p[k], &self.p[(k + 1) % 3]);
            let s = dot3(&self.n, &cross3(&sub3(b, a), &sub3(x, a)));
            match s.cmp(&Rational::ZERO) {
                Ordering::Less => return Inside::Out,
                Ordering::Equal => edge = true,
                Ordering::Greater => {}
            }
        }
        if edge { Inside::Edge } else { Inside::In }
    }

    fn edge(&self, k: usize) -> (&P3, &P3) {
        (&self.p[k], &self.p[(k + 1) % 3])
    }

    /// The squared distance from `x` to the closed triangle.
    fn point_dist2(&self, x: &P3) -> Rational {
        let nn = dot3(&self.n, &self.n);
        let sp = dot3(&self.n, x) - &self.c;
        let foot = sub3(x, &scale3(&self.n, &(&sp / &nn)));
        if self.contains(&foot) != Inside::Out {
            return &sp * &sp / nn;
        }
        let mut best = point_seg_dist2(x, &self.p[0], &self.p[1]);
        for k in 1..3 {
            let (a, b) = self.edge(k);
            let d = point_seg_dist2(x, a, b);
            if d < best {
                best = d;
            }
        }
        best
    }

    /// The segment `a b` meets the triangle at a point off the triangle's
    /// plane or through it (a segment in the plane is left to the edge
    /// and corner distances).
    fn pierced_by(&self, a: &P3, b: &P3) -> bool {
        let sa = dot3(&self.n, a) - &self.c;
        let sb = dot3(&self.n, b) - &self.c;
        let (ga, gb) = (sa.cmp(&Rational::ZERO), sb.cmp(&Rational::ZERO));
        if ga == gb {
            return false;
        }
        let t = &sa / (&sa - &sb);
        self.contains(&lerp3(a, b, &t)) != Inside::Out
    }
}

/// The squared distance between two closed triangles: zero if an edge of
/// one meets the other, else the least of the corner–triangle and
/// edge–edge distances.
fn tri_dist2(x: &Tri, y: &Tri) -> Rational {
    for k in 0..3 {
        let (a, b) = x.edge(k);
        if y.pierced_by(a, b) {
            return Rational::ZERO;
        }
        let (a, b) = y.edge(k);
        if x.pierced_by(a, b) {
            return Rational::ZERO;
        }
    }
    let mut best: Option<Rational> = None;
    let mut keep = |d: Rational| {
        if best.as_ref().is_none_or(|b| d < *b) {
            best = Some(d);
        }
    };
    for k in 0..3 {
        keep(y.point_dist2(&x.p[k]));
        keep(x.point_dist2(&y.p[k]));
        for j in 0..3 {
            let (a, b) = x.edge(k);
            let (c, d) = y.edge(j);
            keep(seg_seg_dist2(a, b, c, d));
        }
    }
    best.unwrap_or(Rational::ZERO)
}

/// The squared distance between two closed faces.
fn face_dist2(ba: &ExactBody, fa: &ExactFace, bb: &ExactBody, fb: &ExactFace) -> Rational {
    let mut best: Option<Rational> = None;
    let tb: Vec<Tri> = fb.triangles.iter().map(|t| bb.tri(t)).collect();
    for ta in &fa.triangles {
        let x = ba.tri(ta);
        for y in &tb {
            let d = tri_dist2(&x, y);
            if d == 0 {
                return d;
            }
            if best.as_ref().is_none_or(|b| d < *b) {
                best = Some(d);
            }
        }
    }
    best.unwrap_or(Rational::ZERO)
}

/// How two faces at distance zero meet ([`Kind`]).
fn meet(ba: &ExactBody, fa: &ExactFace, bb: &ExactBody, fb: &ExactFace) -> Kind {
    let line = cross3(&fa.normal, &fb.normal);
    if line.iter().all(|x| *x == 0) {
        // Parallel at distance zero: one plane.
        let opposed = dot3(&fa.normal, &fb.normal) < 0;
        let axis = largest(&fa.normal);
        let flip = fa.normal[axis] < 0;
        let flat = |body: &ExactBody, t: &[u32; 3]| {
            let mut p = t.map(|k| drop_axis(&body.verts[k as usize], axis));
            if flip {
                p.swap(1, 2);
            }
            p.to_vec()
        };
        let mut pieces = Vec::new();
        let mut area2d_sum = Rational::ZERO;
        for ta in &fa.triangles {
            let pa = flat(ba, ta);
            for tb in &fb.triangles {
                let mut pb = flat(bb, tb);
                if orient2(&pb[0], &pb[1], &pb[2]) < 0 {
                    pb.swap(1, 2);
                }
                let poly = clip_convex(&pb, &pa);
                let a = area2d(&poly);
                if a <= 0 {
                    continue;
                }
                area2d_sum += a;
                let mut piece: Vec<P3> = poly
                    .iter()
                    .map(|y| lift_axis(y, &fa.normal, &fa.offset, axis))
                    .collect();
                if flip {
                    piece.reverse();
                }
                pieces.push(piece);
            }
        }
        // The projection onto the axis plane scales areas by
        // `|n_axis|/|n|`: area² = (A₂/2)²·|n|²/n_axis².
        let half = area2d_sum / Rational::from(2);
        let n2 = dot3(&fa.normal, &fa.normal);
        let area2 = &half * &half * n2 / (&fa.normal[axis] * &fa.normal[axis]);
        return Kind::Coplanar {
            opposed,
            pieces,
            area2,
        };
    }
    let mut runs: Vec<(Rational, Rational, P3, P3)> = Vec::new();
    let tb: Vec<Tri> = fb.triangles.iter().map(|t| bb.tri(t)).collect();
    for ta in &fa.triangles {
        let x = ba.tri(ta);
        let Some(ca) = plane_cut(&x.p, &fb.normal, &fb.offset, &line) else {
            continue;
        };
        for y in &tb {
            let Some(cb) = plane_cut(&y.p, &fa.normal, &fa.offset, &line) else {
                continue;
            };
            let (lo, plo) = if ca.0 >= cb.0 {
                (&ca.0, &ca.1)
            } else {
                (&cb.0, &cb.1)
            };
            let (hi, phi) = if ca.2 <= cb.2 {
                (&ca.2, &ca.3)
            } else {
                (&cb.2, &cb.3)
            };
            if lo < hi {
                runs.push((lo.clone(), hi.clone(), plo.clone(), phi.clone()));
            }
        }
    }
    runs.sort_by(|x, y| x.0.cmp(&y.0).then_with(|| x.1.cmp(&y.1)));
    let mut merged: Vec<(Rational, Rational, P3, P3)> = Vec::new();
    for r in runs {
        match merged.last_mut() {
            Some(last) if r.0 <= last.1 => {
                if r.1 > last.1 {
                    last.1 = r.1;
                    last.3 = r.3;
                }
            }
            _ => merged.push(r),
        }
    }
    let crosses = |body: &ExactBody, f: &ExactFace, n: &P3, c: &Rational| {
        let (mut above, mut below) = (false, false);
        for &k in &f.corners {
            match dot3(n, &body.verts[k as usize]).cmp(c) {
                Ordering::Greater => above = true,
                Ordering::Less => below = true,
                Ordering::Equal => {}
            }
        }
        above && below
    };
    Kind::Meet {
        segments: merged.into_iter().map(|r| [r.2, r.3]).collect(),
        a_crosses: crosses(ba, fa, &fb.normal, &fb.offset),
        b_crosses: crosses(bb, fb, &fa.normal, &fa.offset),
    }
}

/// The triangle `p` cut by the plane `n·x = c` (not its own): the
/// extreme points of the cut along `line`, as `(t_lo, p_lo, t_hi, p_hi)`
/// with `t = line·x`; `None` if the triangle misses the plane.
fn plane_cut(p: &[P3; 3], n: &P3, c: &Rational, line: &P3) -> Option<(Rational, P3, Rational, P3)> {
    let s: Vec<Rational> = p.iter().map(|x| dot3(n, x) - c).collect();
    let mut pts: Vec<P3> = Vec::new();
    for k in 0..3 {
        let j = (k + 1) % 3;
        if s[k] == 0 {
            pts.push(p[k].clone());
        } else if s[j] != 0 && (s[k] > 0) != (s[j] > 0) {
            let t = &s[k] / (&s[k] - &s[j]);
            pts.push(lerp3(&p[k], &p[j], &t));
        }
    }
    let mut it = pts.into_iter();
    let first = it.next()?;
    let t0 = dot3(line, &first);
    let mut out = (t0.clone(), first.clone(), t0, first);
    for x in it {
        let t = dot3(line, &x);
        if t < out.0 {
            out.0 = t.clone();
            out.1 = x.clone();
        }
        if t > out.2 {
            out.2 = t;
            out.3 = x;
        }
    }
    Some(out)
}

/// A body's vertices projected along `ℓ` onto the plane of `axis`.
struct Projected {
    pts: Vec<P2>,
}

impl Projected {
    fn new(body: &ExactBody, l: &P3, axis: usize) -> Projected {
        let (i, j) = ((axis + 1) % 3, (axis + 2) % 3);
        let ri = &l[i] / &l[axis];
        let rj = &l[j] / &l[axis];
        let pts = body
            .verts
            .iter()
            .map(|x| [&x[i] - &(&x[axis] * &ri), &x[j] - &(&x[axis] * &rj)])
            .collect();
        Projected { pts }
    }

    /// The triangle projected, counter-clockwise.
    fn triangle(&self, t: &[u32; 3]) -> Vec<P2> {
        let mut p: Vec<P2> = t.iter().map(|&k| self.pts[k as usize].clone()).collect();
        if orient2(&p[0], &p[1], &p[2]) < 0 {
            p.swap(1, 2);
        }
        p
    }
}

/// The point of the plane `n·x = c` projecting along `ℓ` to `y`.
fn lift(y: &P2, n: &P3, c: &Rational, l: &P3, axis: usize) -> P3 {
    let mut base = zero3();
    base[(axis + 1) % 3] = y[0].clone();
    base[(axis + 2) % 3] = y[1].clone();
    let t = (c - dot3(n, &base)) / dot3(n, l);
    add3(&base, &scale3(l, &t))
}

/// The point of the plane `n·x = c` over `y` in the plane of `axis`.
fn lift_axis(y: &P2, n: &P3, c: &Rational, axis: usize) -> P3 {
    let mut l = zero3();
    l[axis] = Rational::from(1);
    lift(y, n, c, &l, axis)
}

/// `subject` clipped to the counter-clockwise convex `clip`
/// (Sutherland–Hodgman), exactly.
fn clip_convex(subject: &[P2], clip: &[P2]) -> Vec<P2> {
    let mut out: Vec<P2> = subject.to_vec();
    for k in 0..clip.len() {
        if out.is_empty() {
            break;
        }
        let (e0, e1) = (&clip[k], &clip[(k + 1) % clip.len()]);
        let input = std::mem::take(&mut out);
        let o: Vec<Rational> = input.iter().map(|p| orient2(e0, e1, p)).collect();
        for i in 0..input.len() {
            let j = (i + 1) % input.len();
            let (oi, oj) = (&o[i], &o[j]);
            if *oi >= 0 {
                push_new2(&mut out, input[i].clone());
            }
            if (*oi > 0 && *oj < 0) || (*oi < 0 && *oj > 0) {
                let t = oi / (oi - oj);
                let p = [
                    &input[i][0] + &(&(&input[j][0] - &input[i][0]) * &t),
                    &input[i][1] + &(&(&input[j][1] - &input[i][1]) * &t),
                ];
                push_new2(&mut out, p);
            }
        }
        if out.len() > 1 && out.first() == out.last() {
            out.pop();
        }
    }
    out
}

/// Twice the signed area of a polygon.
fn area2d(p: &[P2]) -> Rational {
    let mut a = Rational::ZERO;
    for k in 0..p.len() {
        let (x, y) = (&p[k], &p[(k + 1) % p.len()]);
        a += &x[0] * &y[1] - &x[1] * &y[0];
    }
    a
}

/// The boxes of two polygons meet (closed).
fn boxes_meet2(x: &[P2], y: &[P2]) -> bool {
    (0..2).all(|c| {
        let (xl, xh) = extent(x.iter().map(|p| &p[c]));
        let (yl, yh) = extent(y.iter().map(|p| &p[c]));
        xl <= yh && yl <= xh
    })
}

fn extent<'a>(mut it: impl Iterator<Item = &'a Rational>) -> (&'a Rational, &'a Rational) {
    let first = it.next().expect("a polygon has corners");
    let (mut lo, mut hi) = (first, first);
    for x in it {
        if x < lo {
            lo = x;
        }
        if x > hi {
            hi = x;
        }
    }
    (lo, hi)
}

/// A lower bound on the squared distance between two boxes, exactly.
fn box_gap2(alo: &P3, ahi: &P3, blo: &P3, bhi: &P3) -> Rational {
    let mut g = Rational::ZERO;
    for c in 0..3 {
        let d = if blo[c] > ahi[c] {
            &blo[c] - &ahi[c]
        } else if alo[c] > bhi[c] {
            &alo[c] - &bhi[c]
        } else {
            continue;
        };
        g += &d * &d;
    }
    g
}

/// `ell` exactly and its largest component's axis (the first on a tie).
fn direction(ell: [f64; 3]) -> Result<(P3, usize), OracleError> {
    if !ell.iter().all(|x| x.is_finite()) || ell.iter().all(|&x| x == 0.0) {
        return Err(OracleError::BadInput);
    }
    let l = ell.map(q);
    let axis = largest(&l);
    Ok((l, axis))
}

/// The axis of `v`'s largest component in magnitude, the first on a tie.
fn largest(v: &P3) -> usize {
    let mag = |x: &Rational| if *x < 0 { -x } else { x.clone() };
    let mut k = 0;
    for c in 1..3 {
        if mag(&v[c]) > mag(&v[k]) {
            k = c;
        }
    }
    k
}

/// The point without coordinate `axis`: the next two in cyclic order, so
/// a polygon counter-clockwise about a normal with a positive `axis`
/// component stays counter-clockwise.
fn drop_axis(p: &P3, axis: usize) -> P2 {
    [p[(axis + 1) % 3].clone(), p[(axis + 2) % 3].clone()]
}

fn point_seg_dist2(p: &P3, a: &P3, b: &P3) -> Rational {
    let ab = sub3(b, a);
    let l = dot3(&ab, &ab);
    if l == 0 {
        return dist2(p, a);
    }
    let t = dot3(&sub3(p, a), &ab);
    if t <= 0 {
        return dist2(p, a);
    }
    if t >= l {
        return dist2(p, b);
    }
    dist2(p, &add3(a, &scale3(&ab, &(t / l))))
}

/// The squared distance between the segments `p1 q1` and `p2 q2`: the
/// least of the ends' distances to the other segment and, where the
/// lines' closest points lie inside both, theirs.
fn seg_seg_dist2(p1: &P3, q1: &P3, p2: &P3, q2: &P3) -> Rational {
    let mut best = point_seg_dist2(p1, p2, q2);
    for d in [
        point_seg_dist2(q1, p2, q2),
        point_seg_dist2(p2, p1, q1),
        point_seg_dist2(q2, p1, q1),
    ] {
        if d < best {
            best = d;
        }
    }
    let (d1, d2) = (sub3(q1, p1), sub3(q2, p2));
    let r = sub3(p1, p2);
    let (a, e, b) = (dot3(&d1, &d1), dot3(&d2, &d2), dot3(&d1, &d2));
    let (c, f) = (dot3(&d1, &r), dot3(&d2, &r));
    let den = &a * &e - &b * &b;
    if den > 0 {
        let s = (&b * &f - &c * &e) / &den;
        let t = (&a * &f - &b * &c) / &den;
        if s > 0 && s < 1 && t > 0 && t < 1 {
            let d = dist2(&add3(p1, &scale3(&d1, &s)), &add3(p2, &scale3(&d2, &t)));
            if d < best {
                best = d;
            }
        }
    }
    best
}

fn push_new(v: &mut Vec<P3>, p: P3) {
    if !v.contains(&p) {
        v.push(p);
    }
}

fn push_new2(v: &mut Vec<P2>, p: P2) {
    if v.last() != Some(&p) {
        v.push(p);
    }
}

pub(crate) fn q(x: f64) -> Rational {
    Rational::try_from(x).expect("the rational oracle takes finite inputs")
}

fn zero3() -> P3 {
    [Rational::ZERO, Rational::ZERO, Rational::ZERO]
}

pub(crate) fn add3(a: &P3, b: &P3) -> P3 {
    [&a[0] + &b[0], &a[1] + &b[1], &a[2] + &b[2]]
}

pub(crate) fn sub3(a: &P3, b: &P3) -> P3 {
    [&a[0] - &b[0], &a[1] - &b[1], &a[2] - &b[2]]
}

pub(crate) fn scale3(a: &P3, s: &Rational) -> P3 {
    [&a[0] * s, &a[1] * s, &a[2] * s]
}

pub(crate) fn dot3(a: &P3, b: &P3) -> Rational {
    &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2]
}

fn cross3(a: &P3, b: &P3) -> P3 {
    [
        &a[1] * &b[2] - &a[2] * &b[1],
        &a[2] * &b[0] - &a[0] * &b[2],
        &a[0] * &b[1] - &a[1] * &b[0],
    ]
}

fn lerp3(a: &P3, b: &P3, t: &Rational) -> P3 {
    add3(a, &scale3(&sub3(b, a), t))
}

/// The squared distance between two points.
pub fn dist2(a: &P3, b: &P3) -> Rational {
    let d = sub3(a, b);
    dot3(&d, &d)
}

fn sub2(a: &P2, b: &P2) -> P2 {
    [&a[0] - &b[0], &a[1] - &b[1]]
}

fn dot2(a: &P2, b: &P2) -> Rational {
    &a[0] * &b[0] + &a[1] * &b[1]
}

fn orient2(a: &P2, b: &P2, c: &P2) -> Rational {
    (&b[0] - &a[0]) * (&c[1] - &a[1]) - (&b[1] - &a[1]) * (&c[0] - &a[0])
}

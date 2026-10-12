//! Where a plane meets a solid's curves and faces, for sketches' links:
//! the points where it crosses a chain of conics ([`crossings`]), the
//! curves where it cuts a face ([`face_section`]), and places along a
//! chain ([`sample`]), which a projection maps onto the plane.
//!
//! A plane is a point on it and its unit normal. A conic's signed
//! distance from it is a rational quadratic whose numerator, a quadratic
//! in the parameter, is solved for its roots: exact but for rounding. A
//! face's section needs the solid's faces, and every solid of the facade
//! is empty: it finds none.

use glam::DVec3;

use crate::Solid;
use crate::patch::Conic3;

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
/// its curves, each of places exactly on the face. Every solid of the
/// facade is empty, so the plane misses it: no curves.
pub fn face_section(
    _solid: &Solid,
    _tris: &[u32],
    _plane: &Plane,
    _weld: f64,
) -> Result<Vec<Section>, SectionError> {
    Ok(Vec::new())
}

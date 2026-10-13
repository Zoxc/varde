//! Forms: the analytic surfaces a tiled surface can be exactly, each
//! named by its canonical key ([`FormKey`]), with its implicit equation.
//! So far the plane, the circular cylinder and the sphere.
//!
//! **Keys.** A key holds a form's geometric parameters only, normalised;
//! a face's orientation is not part of it:
//! - plane ([`PlaneKey`]): the unit normal `n` and the offset `d` of
//!   `n·x = d`;
//! - cylinder ([`CylinderKey`]): the unit axis direction, the foot point
//!   (the axis's point nearest the world origin) and the radius;
//! - sphere ([`SphereKey`]): the centre, the radius and the unit axis
//!   direction the caller chose (it fixes the sphere's lattice: poles and
//!   meridians).
//!
//! Directions follow the **sign rule**: the first non-zero component is
//! positive. Each parameter is the exact canonical value of the inputs,
//! rounded once to the nearest `f64` (ties to even): the foot point is
//! rational in the inputs (`p − (p·a)·a/|a|²`), computed exactly in
//! rationals; unit directions and the offset are a rational times a
//! square root, rounded exactly by comparing with the neighbouring
//! midpoints squared. A zero is stored as `+0` (a sphere's centre given
//! with `−0`, or a tiny value rounded to zero), and the sign rule holds
//! for the stored direction: where a direction's leading components
//! round to zero and the first non-zero one left is negative, the stored
//! direction is negated (with a plane's offset, the same plane). So two
//! constructions of one exact form (a point elsewhere on the axis, a
//! scaled or reversed direction) give the same bits, and keys compare
//! bit for bit: equality
//! and the total order ([`Ord`]: plane, cylinder, sphere, then the
//! parameters in their listed order by `f64::total_cmp`) read the stored
//! values only. The lattice a form's surfaces lie on is a fixed function
//! of the stored key ([`super::lattice`]).
//!
//! **Implicits.** [`FormKey::implicit`] evaluates the form's implicit
//! polynomial in the stored key's values at a point, at any precision
//! level or exactly ([`Scalar`]): zero on the form, its sign telling the
//! sides apart.

use std::cmp::Ordering;

use malachite::Rational;
use malachite::base::num::basic::traits::One;
use varde_decide::bezier::Scalar;

use super::round::{nearest, nearest_root, q};

/// Why a key can't be made of the inputs given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormError {
    /// An input is not finite.
    NotFinite,
    /// A direction is zero.
    ZeroDirection,
    /// A radius is not positive.
    Radius,
    /// A key parameter, rounded, is not finite.
    Range,
}

/// A plane's key: `n·x = d`, `n` the unit normal (sign rule).
#[derive(Clone, Copy, Debug)]
pub struct PlaneKey {
    normal: [f64; 3],
    offset: f64,
}

/// A circular cylinder's key: the unit axis direction (sign rule), the
/// axis's point nearest the world origin, the radius.
#[derive(Clone, Copy, Debug)]
pub struct CylinderKey {
    axis: [f64; 3],
    foot: [f64; 3],
    radius: f64,
}

/// A sphere's key: the centre, the radius and the unit axis direction
/// (sign rule).
#[derive(Clone, Copy, Debug)]
pub struct SphereKey {
    centre: [f64; 3],
    radius: f64,
    axis: [f64; 3],
}

/// The canonical key of a form (module docs). Equal and ordered by its
/// stored values, bit for bit.
#[derive(Clone, Copy, Debug)]
pub enum FormKey {
    Plane(PlaneKey),
    Cylinder(CylinderKey),
    Sphere(SphereKey),
}

impl PlaneKey {
    /// The plane through `point` with the normal `normal` (any length,
    /// either sense).
    pub fn new(normal: [f64; 3], point: [f64; 3]) -> Result<PlaneKey, FormError> {
        finite(&normal)?;
        finite(&point)?;
        let n = signed_direction(&normal)?;
        let inv = Rational::ONE / dot(&n, &n);
        let p = point.map(q);
        let normal = [0, 1, 2].map(|k| nearest_root(&n[k], &inv));
        let offset = nearest_root(&dot(&n, &p), &inv);
        stored(&normal)?;
        stored(&[offset])?;
        let (normal, flip) = sign_rule(normal);
        let offset = zero(if flip { -offset } else { offset });
        Ok(PlaneKey { normal, offset })
    }

    /// The unit normal `n`, first non-zero component positive.
    #[inline]
    pub fn normal(&self) -> [f64; 3] {
        self.normal
    }

    /// The offset `d` of `n·x = d`.
    #[inline]
    pub fn offset(&self) -> f64 {
        self.offset
    }
}

impl CylinderKey {
    /// The cylinder of radius `radius` about the line through `point`
    /// along `axis` (any length, either sense).
    pub fn new(point: [f64; 3], axis: [f64; 3], radius: f64) -> Result<CylinderKey, FormError> {
        finite(&point)?;
        let (axis, a) = unit(&axis)?;
        let radius = positive(radius)?;
        let p = point.map(q);
        let t = dot(&p, &a) / dot(&a, &a);
        let foot = [0, 1, 2].map(|k| zero(nearest(&(&p[k] - &t * &a[k]))));
        stored(&foot)?;
        Ok(CylinderKey { axis, foot, radius })
    }

    /// The unit axis direction, first non-zero component positive.
    #[inline]
    pub fn axis(&self) -> [f64; 3] {
        self.axis
    }

    /// The axis's point nearest the world origin.
    #[inline]
    pub fn foot(&self) -> [f64; 3] {
        self.foot
    }

    /// The radius.
    #[inline]
    pub fn radius(&self) -> f64 {
        self.radius
    }
}

impl SphereKey {
    /// The sphere of radius `radius` about `centre`, its lattice's poles
    /// on the line through the centre along `axis` (any length, either
    /// sense).
    pub fn new(centre: [f64; 3], radius: f64, axis: [f64; 3]) -> Result<SphereKey, FormError> {
        finite(&centre)?;
        let radius = positive(radius)?;
        let (axis, _) = unit(&axis)?;
        Ok(SphereKey {
            centre: centre.map(zero),
            radius,
            axis,
        })
    }

    /// The centre.
    #[inline]
    pub fn centre(&self) -> [f64; 3] {
        self.centre
    }

    /// The radius.
    #[inline]
    pub fn radius(&self) -> f64 {
        self.radius
    }

    /// The unit axis direction, first non-zero component positive.
    #[inline]
    pub fn axis(&self) -> [f64; 3] {
        self.axis
    }
}

impl FormKey {
    /// The form's implicit polynomial at `p`, in the stored key's values
    /// at `p`'s level (exactly for exact `p`):
    /// - plane: `n·p − d`;
    /// - cylinder: `|r|²·|a|² − (r·a)² − ρ²·|a|²`, `r = p − foot`, `a` the
    ///   stored axis (`|a|²` times the squared distance from the axis less
    ///   `ρ²`, so exact without a division);
    /// - sphere: `|p − c|² − ρ²`.
    ///
    /// Zero on the form; negative inside a cylinder or sphere, and on the
    /// side of a plane the normal points away from.
    pub fn implicit<S: Scalar>(&self, p: &[S; 3]) -> S {
        let like = &p[0];
        let c = |x: f64| S::from_f64(x, like);
        let cs = |x: &[f64; 3]| x.map(c);
        match self {
            FormKey::Plane(k) => sdot(&cs(&k.normal), p).sub(&c(k.offset)),
            FormKey::Cylinder(k) => {
                let a = cs(&k.axis);
                let r = ssub(p, &cs(&k.foot));
                let aa = sdot(&a, &a);
                let ra = sdot(&r, &a);
                let rho = c(k.radius);
                sdot(&r, &r)
                    .mul(&aa)
                    .sub(&ra.mul(&ra))
                    .sub(&rho.mul(&rho).mul(&aa))
            }
            FormKey::Sphere(k) => {
                let r = ssub(p, &cs(&k.centre));
                let rho = c(k.radius);
                sdot(&r, &r).sub(&rho.mul(&rho))
            }
        }
    }

    /// The degree of [`implicit`](Self::implicit): 1 for a plane, 2 for
    /// a cylinder or sphere.
    pub fn implicit_degree(&self) -> u32 {
        match self {
            FormKey::Plane(_) => 1,
            FormKey::Cylinder(_) | FormKey::Sphere(_) => 2,
        }
    }

    /// The kind's place in the order, then the stored values in order.
    fn parts(&self) -> (u8, [f64; 7]) {
        match self {
            FormKey::Plane(k) => {
                let [a, b, c] = k.normal;
                (0, [a, b, c, k.offset, 0.0, 0.0, 0.0])
            }
            FormKey::Cylinder(k) => {
                let ([a, b, c], [d, e, f]) = (k.axis, k.foot);
                (1, [a, b, c, d, e, f, k.radius])
            }
            FormKey::Sphere(k) => {
                let ([a, b, c], [d, e, f]) = (k.centre, k.axis);
                (2, [a, b, c, k.radius, d, e, f])
            }
        }
    }
}

impl From<PlaneKey> for FormKey {
    fn from(k: PlaneKey) -> FormKey {
        FormKey::Plane(k)
    }
}

impl From<CylinderKey> for FormKey {
    fn from(k: CylinderKey) -> FormKey {
        FormKey::Cylinder(k)
    }
}

impl From<SphereKey> for FormKey {
    fn from(k: SphereKey) -> FormKey {
        FormKey::Sphere(k)
    }
}

/// Keys order by kind (plane, cylinder, sphere), then by their stored
/// values in the order the module docs list them, each by
/// `f64::total_cmp` (so `−0 < +0`): a total order on the bits.
impl Ord for FormKey {
    fn cmp(&self, o: &Self) -> Ordering {
        let (ka, a) = self.parts();
        let (kb, b) = o.parts();
        ka.cmp(&kb).then_with(|| {
            a.iter()
                .zip(&b)
                .map(|(x, y)| x.total_cmp(y))
                .find(|c| c.is_ne())
                .unwrap_or(Ordering::Equal)
        })
    }
}

impl PartialOrd for FormKey {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// Keys are equal when their kinds and stored values are, bit for bit.
impl PartialEq for FormKey {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o).is_eq()
    }
}

impl Eq for FormKey {}

fn finite(x: &[f64]) -> Result<(), FormError> {
    if x.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(FormError::NotFinite)
    }
}

fn stored(x: &[f64]) -> Result<(), FormError> {
    finite(x).map_err(|_| FormError::Range)
}

fn positive(r: f64) -> Result<f64, FormError> {
    if !r.is_finite() {
        Err(FormError::NotFinite)
    } else if r > 0.0 {
        Ok(r)
    } else {
        Err(FormError::Radius)
    }
}

/// `d` exactly, negated if its first non-zero component is negative.
fn signed_direction(d: &[f64; 3]) -> Result<[Rational; 3], FormError> {
    finite(d)?;
    let first = d
        .iter()
        .find(|x| **x != 0.0)
        .ok_or(FormError::ZeroDirection)?;
    let d = d.map(q);
    Ok(if *first < 0.0 { d.map(|x| -x) } else { d })
}

/// `d` normalised under the sign rule and rounded, with the exact
/// signed `d`.
fn unit(d: &[f64; 3]) -> Result<([f64; 3], [Rational; 3]), FormError> {
    let a = signed_direction(d)?;
    let inv = Rational::ONE / dot(&a, &a);
    let u = [0, 1, 2].map(|k| nearest_root(&a[k], &inv));
    stored(&u)?;
    Ok((sign_rule(u).0, a))
}

/// The rounded direction `u` with `+0` for zeros, negated if its first
/// non-zero component is negative (only where the exact direction's
/// leading components rounded to zero); whether it was negated.
fn sign_rule(u: [f64; 3]) -> ([f64; 3], bool) {
    let flip = u.iter().find(|x| **x != 0.0).is_some_and(|x| *x < 0.0);
    (u.map(|x| zero(if flip { -x } else { x })), flip)
}

/// `x`, with `+0` for `−0`.
fn zero(x: f64) -> f64 {
    if x == 0.0 { 0.0 } else { x }
}

pub(crate) fn dot(a: &[Rational; 3], b: &[Rational; 3]) -> Rational {
    &a[0] * &b[0] + &a[1] * &b[1] + &a[2] * &b[2]
}

fn sdot<S: Scalar>(a: &[S; 3], b: &[S; 3]) -> S {
    a[0].mul(&b[0]).add(&a[1].mul(&b[1])).add(&a[2].mul(&b[2]))
}

fn ssub<S: Scalar>(a: &[S; 3], b: &[S; 3]) -> [S; 3] {
    [0, 1, 2].map(|k| a[k].sub(&b[k]))
}

#[cfg(test)]
mod tests;

use std::fmt::Debug;
use std::ops::{Add, Div, Mul, Sub};

use glam::{DVec2, DVec3, DVec4};

use super::{Bounds, PatchError, check_point, check_weight};

/// A point the curves can be built on: [`DVec2`] or [`DVec3`].
pub trait Point:
    Copy
    + Debug
    + PartialEq
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<f64, Output = Self>
    + Div<f64, Output = Self>
    + sealed::Sealed
{
    /// The homogeneous form `(w·p, w)`, one coordinate longer.
    type Hom: Copy
        + Debug
        + PartialEq
        + Add<Output = Self::Hom>
        + Sub<Output = Self::Hom>
        + Mul<f64, Output = Self::Hom>;

    /// `(self·w, w)`.
    fn hom(self, w: f64) -> Self::Hom;
    /// The two parts of a homogeneous point, `(w·p, w)`, as they are.
    fn parts(h: Self::Hom) -> (Self, f64);
    fn min(self, other: Self) -> Self;
    fn max(self, other: Self) -> Self;
    /// The largest absolute coordinate.
    fn max_abs(self) -> f64;
    fn is_finite(self) -> bool;
    fn dot(self, other: Self) -> f64;
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for glam::DVec2 {}
    impl Sealed for glam::DVec3 {}
}

impl Point for DVec2 {
    type Hom = DVec3;

    fn hom(self, w: f64) -> DVec3 {
        (self * w).extend(w)
    }
    fn parts(h: DVec3) -> (DVec2, f64) {
        (h.truncate(), h.z)
    }
    fn min(self, other: Self) -> Self {
        DVec2::min(self, other)
    }
    fn max(self, other: Self) -> Self {
        DVec2::max(self, other)
    }
    fn max_abs(self) -> f64 {
        self.abs().max_element()
    }
    fn is_finite(self) -> bool {
        DVec2::is_finite(self)
    }
    fn dot(self, other: Self) -> f64 {
        DVec2::dot(self, other)
    }
}

impl Point for DVec3 {
    type Hom = DVec4;

    fn hom(self, w: f64) -> DVec4 {
        (self * w).extend(w)
    }
    fn parts(h: DVec4) -> (DVec3, f64) {
        (h.truncate(), h.w)
    }
    fn min(self, other: Self) -> Self {
        DVec3::min(self, other)
    }
    fn max(self, other: Self) -> Self {
        DVec3::max(self, other)
    }
    fn max_abs(self) -> f64 {
        self.abs().max_element()
    }
    fn is_finite(self) -> bool {
        DVec3::is_finite(self)
    }
    fn dot(self, other: Self) -> f64 {
        DVec3::dot(self, other)
    }
}

/// A rational quadratic curve in the standard form: from `p0` to `p1`,
/// pulled towards the control point `c` by the weight `w` (see the
/// [module](super) docs). Weight 1 with `c` at the midpoint is a straight
/// segment; `cos(θ/2)` with `c` where the end tangents meet is a circular
/// arc of angle `θ`.
///
/// The fields are open, so [`Conic::check`] is what makes one trusted;
/// [`Conic::new`] and every construction here check.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Conic<P> {
    pub p0: P,
    pub c: P,
    pub w: f64,
    pub p1: P,
}

/// A conic in the plane, as profiles are made of.
pub type Conic2 = Conic<DVec2>;
/// A conic in space, as patch edges are.
pub type Conic3 = Conic<DVec3>;

/// The corner `h` as a point, with its homogeneous weight, which must be
/// positive.
pub(super) fn corner<P: Point>(h: P::Hom) -> Result<(P, f64), PatchError> {
    let (x, w) = P::parts(h);
    if w > 0.0 && w.is_finite() {
        Ok((x / w, w))
    } else {
        Err(PatchError::Weight(w))
    }
}

/// The homogeneous edge control point `h` between corners of homogeneous
/// weights `wa` and `wb`, in the standard form: its point and the weight
/// `w / √(wa·wb)`. Symmetric in `wa` and `wb`, so both directions of an
/// edge give the same bits.
pub(super) fn edge<P: Point>(h: P::Hom, wa: f64, wb: f64) -> Result<(P, f64), PatchError> {
    let (x, w) = P::parts(h);
    if w > 0.0 && w.is_finite() {
        Ok((x / w, w / (wa * wb).sqrt()))
    } else {
        Err(PatchError::Weight(w))
    }
}

impl<P: Point> Conic<P> {
    /// The curve, if it passes [`Conic::check`].
    pub fn new(p0: P, c: P, w: f64, p1: P) -> Result<Self, PatchError> {
        let conic = Conic { p0, c, w, p1 };
        conic.check()?;
        Ok(conic)
    }

    /// The straight segment from `p0` to `p1`: the control point at the
    /// midpoint and weight 1.
    pub fn line(p0: P, p1: P) -> Result<Self, PatchError> {
        Self::new(p0, (p0 + p1) * 0.5, 1.0, p1)
    }

    /// Every coordinate finite and within
    /// [`MAX_CONTROL`](super::MAX_CONTROL), and the weight within
    /// [`W_MIN`](super::W_MIN)`..=`[`W_MAX`](super::W_MAX).
    pub fn check(&self) -> Result<(), PatchError> {
        check_point(self.p0)?;
        check_point(self.c)?;
        check_point(self.p1)?;
        check_weight(self.w)
    }

    /// The same curve run from `p1` to `p0`.
    pub fn reversed(&self) -> Self {
        Conic {
            p0: self.p1,
            c: self.c,
            w: self.w,
            p1: self.p0,
        }
    }

    /// The curve moved by `offset`.
    pub fn translated(&self, offset: P) -> Self {
        Conic {
            p0: self.p0 + offset,
            c: self.c + offset,
            w: self.w,
            p1: self.p1 + offset,
        }
    }

    /// The homogeneous control points `(p0, 1)`, `(w·c, w)`, `(p1, 1)`.
    pub fn hom(&self) -> [P::Hom; 3] {
        [self.p0.hom(1.0), self.c.hom(self.w), self.p1.hom(1.0)]
    }

    /// The curve with homogeneous control points `h`, in the standard
    /// form. Every homogeneous weight must be positive, and the result
    /// must pass [`Conic::check`].
    pub fn from_hom(h: [P::Hom; 3]) -> Result<Self, PatchError> {
        let (p0, w0) = corner::<P>(h[0])?;
        let (p1, w1) = corner::<P>(h[2])?;
        let (c, w) = edge::<P>(h[1], w0, w1)?;
        Self::new(p0, c, w, p1)
    }

    /// The point at `t`, in `[0, 1]` along the curve.
    pub fn eval(&self, t: f64) -> P {
        let s = 1.0 - t;
        let (b0, b1, b2) = (s * s, 2.0 * s * t, t * t);
        let d = b0 + b1 * self.w + b2;
        (self.p0 * b0 + self.c * (b1 * self.w) + self.p1 * b2) / d
    }

    /// The point at `t` and the first derivative there.
    pub fn eval_deriv(&self, t: f64) -> (P, P) {
        let [h0, h1, h2] = self.hom();
        let s = 1.0 - t;
        let x = h0 * (s * s) + h1 * (2.0 * s * t) + h2 * (t * t);
        let dx = ((h1 - h0) * s + (h2 - h1) * t) * 2.0;
        let (n, d) = P::parts(x);
        let (dn, dd) = P::parts(dx);
        let p = n / d;
        (p, (dn - p * dd) / d)
    }

    /// The homogeneous blossom `B(s, t)`: `B(t, t)` is the homogeneous
    /// point at `t`, and `B(s, t)` for `s ≠ t` the control point of the
    /// piece between them.
    pub fn blossom(&self, s: f64, t: f64) -> P::Hom {
        let [h0, h1, h2] = self.hom();
        let (s1, t1) = (1.0 - s, 1.0 - t);
        h0 * (s1 * t1) + h1 * (s1 * t + s * t1) + h2 * (s * t)
    }

    /// The homogeneous control points of the two halves split at `t`, by
    /// de Casteljau: `[p0, left control, split point, right control,
    /// p1]`.
    fn split_hom(&self, t: f64) -> [P::Hom; 5] {
        let [h0, h1, h2] = self.hom();
        let s = 1.0 - t;
        let l = h0 * s + h1 * t;
        let r = h1 * s + h2 * t;
        [h0, l, l * s + r * t, r, h2]
    }

    /// [`Self::split_hom`] at `½`, written so that the reversed curve
    /// gives the same bits in the reverse order.
    fn split_half_hom(&self) -> [P::Hom; 5] {
        let [h0, h1, h2] = self.hom();
        let l = (h0 + h1) * 0.5;
        let r = (h1 + h2) * 0.5;
        [h0, l, (l + r) * 0.5, r, h2]
    }

    /// The two pieces either side of `t`, in `(0, 1)`, each in the
    /// standard form. Exact up to rounding: the halves trace the curve.
    ///
    /// At `½` this is [`Self::split_half`]. At other positions the
    /// reversed curve split at `1 - t` rounds differently, so a curve two
    /// patches share must be split once, with the halves handed to both.
    pub fn split(&self, t: f64) -> Result<[Self; 2], PatchError> {
        if t == 0.5 {
            return self.split_half();
        }
        if !(t > 0.0 && t < 1.0) {
            return Err(PatchError::Parameter(t));
        }
        Self::halves(self.split_hom(t))
    }

    /// The two halves at `t = ½`. Symmetric to the last bit: the reversed
    /// curve gives the same two halves, reversed and swapped.
    pub fn split_half(&self) -> Result<[Self; 2], PatchError> {
        Self::halves(self.split_half_hom())
    }

    fn halves(h: [P::Hom; 5]) -> Result<[Self; 2], PatchError> {
        Ok([
            Self::from_hom([h[0], h[1], h[2]])?,
            Self::from_hom([h[2], h[3], h[4]])?,
        ])
    }

    /// The control points, whose convex hull holds the curve.
    pub fn hull(&self) -> [P; 3] {
        [self.p0, self.c, self.p1]
    }

    /// The box around the control points, and so around the curve.
    pub fn bounds(&self) -> Bounds<P> {
        Bounds {
            min: self.p0.min(self.c).min(self.p1),
            max: self.p0.max(self.c).max(self.p1),
        }
    }
}

/// The largest start angle, in radians, an arc may be given: well past
/// any angle a sketch gives, and small enough that `cos` and `sin` stay
/// accurate.
const MAX_ARC_START: f64 = 16.0 * std::f64::consts::PI;

/// The largest sweep of one exact arc: 90°, with a little room for the
/// rounding of a sweep divided into equal parts.
const MAX_ARC_SWEEP: f64 = std::f64::consts::FRAC_PI_2 * (1.0 + 1e-12);

/// An arc of the circle of `radius` around the origin, from angle `start`
/// through `sweep` (counter-clockwise if positive), in the standard form:
/// its ends, its control point where the end tangents meet, and its
/// weight `cos(sweep/2)`.
fn arc_parts(radius: f64, start: f64, sweep: f64) -> Result<([DVec2; 3], f64), PatchError> {
    if !(radius > 0.0 && radius <= super::MAX_CONTROL) {
        return Err(PatchError::Parameter(radius));
    }
    if !(start.is_finite() && start.abs() <= MAX_ARC_START) {
        return Err(PatchError::Parameter(start));
    }
    if !(sweep != 0.0 && sweep.abs() <= MAX_ARC_SWEEP) {
        return Err(PatchError::Parameter(sweep));
    }
    let at = |angle: f64, r: f64| DVec2::new(angle.cos(), angle.sin()) * r;
    let half = sweep * 0.5;
    let w = half.cos();
    Ok((
        [
            at(start, radius),
            at(start + half, radius / w),
            at(start + sweep, radius),
        ],
        w,
    ))
}

impl Conic2 {
    /// The exact arc of the circle of `radius` around `center`, from the
    /// angle `start` (radians, from +x towards +y) through `sweep`, which
    /// is at most 90° either way and not zero.
    pub fn arc(center: DVec2, radius: f64, start: f64, sweep: f64) -> Result<Self, PatchError> {
        let ([p0, c, p1], w) = arc_parts(radius, start, sweep)?;
        Self::new(center + p0, center + c, w, center + p1)
    }
}

impl Conic3 {
    /// The exact arc of the circle of `radius` around `center` in the
    /// plane of the orthonormal axes `x` and `y`, from the angle `start`
    /// (radians, from `x` towards `y`) through `sweep`, which is at most
    /// 90° either way and not zero. With axes that aren't orthonormal it
    /// is the matching arc of an ellipse.
    pub fn arc(
        center: DVec3,
        x: DVec3,
        y: DVec3,
        radius: f64,
        start: f64,
        sweep: f64,
    ) -> Result<Self, PatchError> {
        let ([p0, c, p1], w) = arc_parts(radius, start, sweep)?;
        let place = |p: DVec2| center + x * p.x + y * p.y;
        Self::new(place(p0), place(c), w, place(p1))
    }
}

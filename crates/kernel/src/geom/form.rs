//! Forms: the analytic surfaces a tiled surface can be exactly, each
//! named by its canonical key ([`FormKey`]), with its implicit equation.
//!
//! **Keys.** A key holds a form's geometric parameters only, normalised;
//! a face's orientation is not part of it:
//! - plane ([`PlaneKey`]): the unit normal `n` and the offset `d` of
//!   `n·x = d`;
//! - circular cylinder ([`CylinderKey`]): the unit axis direction, the
//!   foot point (the axis's point nearest the world origin) and the
//!   radius;
//! - conic-chain cylinder ([`ConicCylinderKey`]): the unit axis direction
//!   and the profile, a chain of rational quadratic spans in the
//!   canonical frame of the plane through the world origin perpendicular
//!   to the axis ([`Profile`]);
//! - cone ([`ConeKey`]): the apex, the unit axis direction pointing into
//!   the nappe used (no sign rule), and the half-angle by its cosine and
//!   sine;
//! - sphere ([`SphereKey`]): the centre, the radius and the unit axis
//!   direction the caller chose (it fixes the sphere's lattice: poles and
//!   meridians);
//! - torus ([`TorusKey`]): the centre, the unit axis direction, the major
//!   radius `R` and the minor radius `r` (`R ≠ r`: a horn torus is a
//!   revolved conic);
//! - revolved conic ([`RevolvedKey`]): the axis as a cylinder's (unit
//!   direction and foot point) and the profile in the canonical meridian
//!   frame: `(ρ, z, w)`, `ρ` the distance from the axis, `z` along the
//!   axis from the foot point;
//! - affine image ([`AffineKey`]): the source form's key and the map, as
//!   given.
//!
//! Directions follow the **sign rule**, the cone's excepted: the first
//! non-zero component is positive. Each parameter is the exact canonical
//! value of the inputs rounded once to the nearest `f64`, ties to even,
//! by its route ([`super::formkey`]): foot points (`p − (p·a)·a/|a|²`)
//! are rational, rounded from their exact value; unit directions, the
//! plane's offset, the cone's cosine and sine and the profiles'
//! coordinates are quadratic irrationals, rounded by the loop on the
//! precision schedule; radii, centres, the apex, `ρ`, weights and the
//! affine map are inputs, taken as they are. A zero is stored as `+0`
//! (a centre given with `−0`, or a tiny value rounded to zero), and the
//! sign rule holds for the stored direction: where a direction's leading
//! components round to zero and the first non-zero one left is negative,
//! the stored direction is negated (with a plane's offset, a profile's
//! first coordinate or `z`, the same form). So two constructions of one
//! exact form (a point elsewhere on the axis or the plane, a scaled or
//! reversed direction, a profile given at another height or reversed)
//! give the same bits, and keys compare bit for bit: equality and the
//! total order ([`Ord`]: by kind in the order listed, then the stored
//! values in their listed order by `f64::total_cmp`, a profile's spans
//! last, an affine image's source after its map) read the stored values
//! only. The lattice a form's surfaces lie on is a fixed function of the
//! stored key ([`super::lattice`]).
//!
//! **Profiles** are chains of spans, each three control points `(x, y,
//! w)` stored affine, consecutive spans sharing their end point. The
//! chain runs in the canonical direction: of the chain and its reverse,
//! the one whose sequence of stored values is less by `f64::total_cmp`.
//! A conic-chain cylinder's profile is the orthogonal projection of the
//! given spans: `x` along `ê₁ = normalise(e_k × a)`, `y` along `â × ê₁`,
//! `e_k` the world axis of least `|a_k|` of the stored direction `a`
//! (ties: the lowest `k`), the lattice frame's rule.
//!
//! **Charges.** Inside an operation a key is built with a meter
//! (`metered`) that charges each parameter's rounding to the item's
//! ledger; running out is the budget's, never a different rounding. A
//! key built with `new` is not charged.
//!
//! **Implicits.** [`FormKey::implicit`] evaluates the form's implicit
//! polynomial in the stored key's values at a point, at any precision
//! level or exactly ([`Scalar`]): zero on the form, its sign telling the
//! sides apart. Forms whose implicit is per tile have none here.

use std::cmp::Ordering;
use std::convert::Infallible;
use std::sync::Arc;

use malachite::Rational;
use malachite::base::num::basic::traits::Zero;
use varde_decide::bezier::Scalar;

use super::curve::Span3;
use super::formkey::{Charge, Expr, KeyMeter, Quadratic, Uncharged, zero};
use super::round::q;
use crate::infra::budget::{Exhausted, Stop};

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
    /// A cone's half-angle is not strictly between 0 and 90°: a part of
    /// it not positive.
    Angle,
    /// A torus's radii are equal (a horn torus, a revolved conic).
    Horn,
    /// A profile is empty, has a weight that isn't positive, or two
    /// consecutive spans don't share their end point.
    Profile,
    /// An affine image of an affine image (compose the maps instead).
    Nested,
    /// An affine map that isn't invertible.
    Singular,
}

/// Why a key can't be made inside an operation: its inputs, or the
/// budget ran out while rounding it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyError<E> {
    Form(FormError),
    Budget(E),
}

impl<E> From<FormError> for KeyError<E> {
    fn from(e: FormError) -> Self {
        KeyError::Form(e)
    }
}

impl<E: From<FormError>> From<KeyError<Exhausted>> for Stop<E> {
    fn from(e: KeyError<Exhausted>) -> Self {
        match e {
            KeyError::Form(e) => Stop::Failed(e.into()),
            KeyError::Budget(e) => Stop::Exhausted(e),
        }
    }
}

type Built<K, C> = Result<K, KeyError<<C as Charge>::Err>>;

/// A key built uncharged.
fn uncharged<K>(
    f: impl FnOnce(&mut KeyMeter<'_, Uncharged>) -> Result<K, KeyError<Infallible>>,
) -> Result<K, FormError> {
    let mut free = Uncharged;
    match f(&mut KeyMeter::new(&mut free)) {
        Ok(k) => Ok(k),
        Err(KeyError::Form(e)) => Err(e),
        Err(KeyError::Budget(never)) => match never {},
    }
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

/// A conic-chain cylinder's key: the unit axis direction (sign rule) and
/// the profile in the canonical frame perpendicular to it.
#[derive(Clone, Debug)]
pub struct ConicCylinderKey {
    axis: [f64; 3],
    profile: Profile,
}

/// A cone's key: the apex, the unit axis direction into the nappe used,
/// and the half-angle's cosine and sine.
#[derive(Clone, Copy, Debug)]
pub struct ConeKey {
    apex: [f64; 3],
    axis: [f64; 3],
    cos: f64,
    sin: f64,
}

/// A sphere's key: the centre, the radius and the unit axis direction
/// (sign rule).
#[derive(Clone, Copy, Debug)]
pub struct SphereKey {
    centre: [f64; 3],
    radius: f64,
    axis: [f64; 3],
}

/// A torus's key: the centre, the unit axis direction (sign rule), the
/// major and the minor radius.
#[derive(Clone, Copy, Debug)]
pub struct TorusKey {
    centre: [f64; 3],
    axis: [f64; 3],
    major: f64,
    minor: f64,
}

/// A revolved conic's key: the unit axis direction (sign rule), the
/// axis's point nearest the world origin, and the profile `(ρ, z, w)` in
/// the meridian frame.
#[derive(Clone, Debug)]
pub struct RevolvedKey {
    axis: [f64; 3],
    foot: [f64; 3],
    profile: Profile,
}

/// An affine image's key: the source form's key and the map `x ↦ M·x +
/// t`, as given.
#[derive(Clone, Debug)]
pub struct AffineKey {
    source: Arc<FormKey>,
    linear: [[f64; 3]; 3],
    shift: [f64; 3],
}

/// One span of a profile: three control points `(x, y, w)`, affine.
pub type Span2 = [[f64; 3]; 3];

/// A profile: a chain of spans in a form's canonical plane frame, in its
/// canonical direction (module docs).
#[derive(Clone, Debug)]
pub struct Profile {
    spans: Arc<[Span2]>,
}

/// The canonical key of a form (module docs). Equal and ordered by its
/// stored values, bit for bit.
#[derive(Clone, Debug)]
pub enum FormKey {
    Plane(PlaneKey),
    Cylinder(CylinderKey),
    ConicCylinder(ConicCylinderKey),
    Cone(ConeKey),
    Sphere(SphereKey),
    Torus(TorusKey),
    Revolved(RevolvedKey),
    Affine(AffineKey),
}

impl PlaneKey {
    /// The plane through `point` with the normal `normal` (any length,
    /// either sense).
    pub fn new(normal: [f64; 3], point: [f64; 3]) -> Result<PlaneKey, FormError> {
        uncharged(|m| PlaneKey::metered(normal, point, m))
    }

    /// [`new`](Self::new), each parameter's rounding charged to `m`.
    pub(crate) fn metered<C: Charge>(
        normal: [f64; 3],
        point: [f64; 3],
        m: &mut KeyMeter<'_, C>,
    ) -> Built<PlaneKey, C> {
        finite(&normal)?;
        finite(&point)?;
        let (n, _) = signed_direction(&normal)?;
        let normal = components(&n, m)?;
        let e = n.map(Expr::Lit);
        let offset = Quadratic::ratio(Expr::dot(&e, &point.map(Expr::Lit)), Expr::dot(&e, &e));
        let offset = m.irrational(&offset).map_err(KeyError::Budget)?;
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
        uncharged(|m| CylinderKey::metered(point, axis, radius, m))
    }

    /// [`new`](Self::new), each parameter's rounding charged to `m`.
    pub(crate) fn metered<C: Charge>(
        point: [f64; 3],
        axis: [f64; 3],
        radius: f64,
        m: &mut KeyMeter<'_, C>,
    ) -> Built<CylinderKey, C> {
        finite(&point)?;
        let radius = positive(radius)?;
        let line = Axis::new(&point, &axis, m)?;
        Ok(CylinderKey {
            axis: line.stored,
            foot: line.foot,
            radius,
        })
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

impl ConicCylinderKey {
    /// The cylinder along `axis` (any length, either sense) over the
    /// chain `spans`: control points `(x, y, z, w)` in space, consecutive
    /// spans sharing their end point, projected along the axis onto the
    /// plane through the world origin perpendicular to it.
    pub fn new(axis: [f64; 3], spans: &[Span3]) -> Result<ConicCylinderKey, FormError> {
        uncharged(|m| ConicCylinderKey::metered(axis, spans, m))
    }

    /// [`new`](Self::new), each parameter's rounding charged to `m`.
    pub(crate) fn metered<C: Charge>(
        axis: [f64; 3],
        spans: &[Span3],
        m: &mut KeyMeter<'_, C>,
    ) -> Built<ConicCylinderKey, C> {
        chain(spans.iter().map(|s| s.map(|p| ([p[0], p[1], p[2]], p[3]))))?;
        let (stored_axis, a) = unit(&axis, m)?;
        // The frame: `U = e_k × a`, `V = a × U`, with `e_k` from the
        // stored direction (lattice rule).
        let k = least(&stored_axis);
        let u: [f64; 3] = match k {
            0 => [0.0, -a[2], a[1]],
            1 => [a[2], 0.0, -a[0]],
            _ => [-a[1], a[0], 0.0],
        };
        let (ae, ue) = (a.map(Expr::Lit), u.map(Expr::Lit));
        let v = [
            ae[1].clone() * ue[2].clone() - ae[2].clone() * ue[1].clone(),
            ae[2].clone() * ue[0].clone() - ae[0].clone() * ue[2].clone(),
            ae[0].clone() * ue[1].clone() - ae[1].clone() * ue[0].clone(),
        ];
        let uu = Expr::dot(&ue, &ue);
        let vv = Expr::dot(&ae, &ae) * uu.clone();
        let mut out = Vec::with_capacity(spans.len());
        for s in spans {
            let mut span = [[0.0; 3]; 3];
            for (c, p) in span.iter_mut().zip(s) {
                let pe = [p[0], p[1], p[2]].map(Expr::Lit);
                let x = Quadratic::ratio(Expr::dot(&pe, &ue), uu.clone());
                let y = Quadratic::ratio(Expr::dot(&pe, &v), vv.clone());
                let x = m.irrational(&x).map_err(KeyError::Budget)?;
                let y = m.irrational(&y).map_err(KeyError::Budget)?;
                *c = [x, y, zero(p[3])];
            }
            stored(span.as_flattened())?;
            out.push(span);
        }
        Ok(ConicCylinderKey {
            axis: stored_axis,
            profile: Profile::canonical(out),
        })
    }

    /// The unit axis direction, first non-zero component positive.
    #[inline]
    pub fn axis(&self) -> [f64; 3] {
        self.axis
    }

    /// The profile in the canonical frame perpendicular to the axis.
    #[inline]
    pub fn profile(&self) -> &Profile {
        &self.profile
    }
}

impl ConeKey {
    /// The cone with apex `apex` opening along `axis` (any length; into
    /// the nappe used), its half-angle the angle whose tangent is
    /// `across/along`, both positive and of any common scale.
    pub fn new(
        apex: [f64; 3],
        axis: [f64; 3],
        along: f64,
        across: f64,
    ) -> Result<ConeKey, FormError> {
        uncharged(|m| ConeKey::metered(apex, axis, along, across, m))
    }

    /// [`new`](Self::new), each parameter's rounding charged to `m`.
    pub(crate) fn metered<C: Charge>(
        apex: [f64; 3],
        axis: [f64; 3],
        along: f64,
        across: f64,
        m: &mut KeyMeter<'_, C>,
    ) -> Built<ConeKey, C> {
        finite(&apex)?;
        finite(&axis)?;
        finite(&[along, across])?;
        if !(along > 0.0 && across > 0.0) {
            return Err(FormError::Angle.into());
        }
        if axis.iter().all(|x| *x == 0.0) {
            return Err(FormError::ZeroDirection.into());
        }
        let a = scaled(axis);
        let dir = components(&a, m)?;
        stored(&dir)?;
        let [h, r] = scaled2([along, across]);
        let hh = Expr::Lit(h) * Expr::Lit(h) + Expr::Lit(r) * Expr::Lit(r);
        let cos = m
            .irrational(&Quadratic::ratio(Expr::Lit(h), hh.clone()))
            .map_err(KeyError::Budget)?;
        let sin = m
            .irrational(&Quadratic::ratio(Expr::Lit(r), hh))
            .map_err(KeyError::Budget)?;
        stored(&[cos, sin])?;
        Ok(ConeKey {
            apex: apex.map(zero),
            axis: dir.map(zero),
            cos,
            sin,
        })
    }

    /// The apex.
    #[inline]
    pub fn apex(&self) -> [f64; 3] {
        self.apex
    }

    /// The unit axis direction, into the nappe used.
    #[inline]
    pub fn axis(&self) -> [f64; 3] {
        self.axis
    }

    /// The half-angle's cosine.
    #[inline]
    pub fn cos(&self) -> f64 {
        self.cos
    }

    /// The half-angle's sine.
    #[inline]
    pub fn sin(&self) -> f64 {
        self.sin
    }
}

impl SphereKey {
    /// The sphere of radius `radius` about `centre`, its lattice's poles
    /// on the line through the centre along `axis` (any length, either
    /// sense).
    pub fn new(centre: [f64; 3], radius: f64, axis: [f64; 3]) -> Result<SphereKey, FormError> {
        uncharged(|m| SphereKey::metered(centre, radius, axis, m))
    }

    /// [`new`](Self::new), each parameter's rounding charged to `m`.
    pub(crate) fn metered<C: Charge>(
        centre: [f64; 3],
        radius: f64,
        axis: [f64; 3],
        m: &mut KeyMeter<'_, C>,
    ) -> Built<SphereKey, C> {
        finite(&centre)?;
        let radius = positive(radius)?;
        let (axis, _) = unit(&axis, m)?;
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

impl TorusKey {
    /// The torus about the line through `centre` along `axis` (any
    /// length, either sense), its tube's centre circle of radius `major`
    /// in the plane through `centre` perpendicular to the axis, the tube
    /// of radius `minor`; `major ≠ minor`.
    pub fn new(
        centre: [f64; 3],
        axis: [f64; 3],
        major: f64,
        minor: f64,
    ) -> Result<TorusKey, FormError> {
        uncharged(|m| TorusKey::metered(centre, axis, major, minor, m))
    }

    /// [`new`](Self::new), each parameter's rounding charged to `m`.
    pub(crate) fn metered<C: Charge>(
        centre: [f64; 3],
        axis: [f64; 3],
        major: f64,
        minor: f64,
        m: &mut KeyMeter<'_, C>,
    ) -> Built<TorusKey, C> {
        finite(&centre)?;
        let (major, minor) = (positive(major)?, positive(minor)?);
        if major == minor {
            return Err(FormError::Horn.into());
        }
        let (axis, _) = unit(&axis, m)?;
        Ok(TorusKey {
            centre: centre.map(zero),
            axis,
            major,
            minor,
        })
    }

    /// The centre.
    #[inline]
    pub fn centre(&self) -> [f64; 3] {
        self.centre
    }

    /// The unit axis direction, first non-zero component positive.
    #[inline]
    pub fn axis(&self) -> [f64; 3] {
        self.axis
    }

    /// The major radius `R`.
    #[inline]
    pub fn major(&self) -> f64 {
        self.major
    }

    /// The minor radius `r`.
    #[inline]
    pub fn minor(&self) -> f64 {
        self.minor
    }
}

impl RevolvedKey {
    /// The surface of revolution about the line through `point` along
    /// `axis` (any length, either sense) of the chain `spans`: control
    /// points `(ρ, z, w)` in a meridian half-plane, `ρ` the distance from
    /// the axis, `z` along `axis` from `point`; consecutive spans sharing
    /// their end point.
    pub fn new(point: [f64; 3], axis: [f64; 3], spans: &[Span2]) -> Result<RevolvedKey, FormError> {
        uncharged(|m| RevolvedKey::metered(point, axis, spans, m))
    }

    /// [`new`](Self::new), each parameter's rounding charged to `m`.
    pub(crate) fn metered<C: Charge>(
        point: [f64; 3],
        axis: [f64; 3],
        spans: &[Span2],
        m: &mut KeyMeter<'_, C>,
    ) -> Built<RevolvedKey, C> {
        finite(&point)?;
        chain(spans.iter().map(|s| s.map(|p| ([p[0], p[1], 0.0], p[2]))))?;
        let line = Axis::new(&point, &axis, m)?;
        // `z` from the foot along the stored axis: `σz + p·â`.
        let a = line.exact.map(Expr::Lit);
        let along = Expr::dot(&point.map(Expr::Lit), &a);
        let aa = Expr::dot(&a, &a);
        let mut out = Vec::with_capacity(spans.len());
        for s in spans {
            let mut span = [[0.0; 3]; 3];
            for (c, p) in span.iter_mut().zip(s) {
                let z = Quadratic {
                    c: Expr::Lit(if line.reversed { -p[1] } else { p[1] }),
                    n: along.clone(),
                    d: aa.clone(),
                };
                let z = m.irrational(&z).map_err(KeyError::Budget)?;
                *c = [zero(p[0]), z, zero(p[2])];
            }
            stored(span.as_flattened())?;
            out.push(span);
        }
        Ok(RevolvedKey {
            axis: line.stored,
            foot: line.foot,
            profile: Profile::canonical(out),
        })
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

    /// The profile `(ρ, z, w)` in the meridian frame.
    #[inline]
    pub fn profile(&self) -> &Profile {
        &self.profile
    }
}

impl AffineKey {
    /// The image of the form `source` under `x ↦ linear·x + shift`
    /// (`linear` by rows, invertible); `source` not itself an affine
    /// image.
    pub fn new(
        source: FormKey,
        linear: [[f64; 3]; 3],
        shift: [f64; 3],
    ) -> Result<AffineKey, FormError> {
        if matches!(source, FormKey::Affine(_)) {
            return Err(FormError::Nested);
        }
        finite(linear.as_flattened())?;
        finite(&shift)?;
        let r = linear.map(|row| row.map(q));
        let det = &r[0][0] * (&r[1][1] * &r[2][2] - &r[1][2] * &r[2][1])
            - &r[0][1] * (&r[1][0] * &r[2][2] - &r[1][2] * &r[2][0])
            + &r[0][2] * (&r[1][0] * &r[2][1] - &r[1][1] * &r[2][0]);
        if det == Rational::ZERO {
            return Err(FormError::Singular);
        }
        Ok(AffineKey {
            source: Arc::new(source),
            linear: linear.map(|row| row.map(zero)),
            shift: shift.map(zero),
        })
    }

    /// The source form's key.
    #[inline]
    pub fn source(&self) -> &FormKey {
        &self.source
    }

    /// The map's linear part, by rows.
    #[inline]
    pub fn linear(&self) -> [[f64; 3]; 3] {
        self.linear
    }

    /// The map's shift.
    #[inline]
    pub fn shift(&self) -> [f64; 3] {
        self.shift
    }
}

impl Profile {
    /// The chain `spans` or its reverse, whichever is the canonical
    /// direction (module docs).
    fn canonical(spans: Vec<Span2>) -> Profile {
        let reversed: Vec<Span2> = spans.iter().rev().map(|s| [s[2], s[1], s[0]]).collect();
        let flat = |s: &[Span2]| s.as_flattened().as_flattened().to_vec();
        let spans = if total_cmp(&flat(&reversed), &flat(&spans)).is_lt() {
            reversed
        } else {
            spans
        };
        Profile {
            spans: spans.into(),
        }
    }

    /// The spans, in the canonical direction.
    #[inline]
    pub fn spans(&self) -> &[Span2] {
        &self.spans
    }

    fn values(&self) -> &[f64] {
        self.spans.as_flattened().as_flattened()
    }
}

/// The most stored values a key holds before its profile and source.
const HEAD: usize = 12;

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
    /// side of a plane the normal points away from. `None` for the other
    /// forms: their implicits are their tiles'.
    pub fn implicit<S: Scalar>(&self, p: &[S; 3]) -> Option<S> {
        let like = &p[0];
        let c = |x: f64| S::from_f64(x, like);
        let cs = |x: &[f64; 3]| x.map(c);
        Some(match self {
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
            _ => return None,
        })
    }

    /// The degree of [`implicit`](Self::implicit): 1 for a plane, 2 for
    /// a cylinder or sphere, `None` where it has none.
    pub fn implicit_degree(&self) -> Option<u32> {
        match self {
            FormKey::Plane(_) => Some(1),
            FormKey::Cylinder(_) | FormKey::Sphere(_) => Some(2),
            _ => None,
        }
    }

    /// Every stored value, in the order keys compare them: the head
    /// values, a profile's spans, an affine image's source.
    pub fn values(&self) -> Vec<f64> {
        let (_, head, n) = self.head();
        let mut out = head[..n].to_vec();
        if let Some(p) = self.profile() {
            out.extend_from_slice(p.values());
        }
        if let FormKey::Affine(k) = self {
            out.extend(k.source.values());
        }
        out
    }

    /// The key's points in space: a foot point, apex or centre (none for
    /// a plane, a conic-chain cylinder or an affine image, whose points
    /// are not in the world frame).
    pub fn points(&self) -> Vec<[f64; 3]> {
        match self {
            FormKey::Cylinder(k) => vec![k.foot],
            FormKey::Revolved(k) => vec![k.foot],
            FormKey::Cone(k) => vec![k.apex],
            FormKey::Sphere(k) => vec![k.centre],
            FormKey::Torus(k) => vec![k.centre],
            FormKey::Plane(_) | FormKey::ConicCylinder(_) | FormKey::Affine(_) => Vec::new(),
        }
    }

    /// The profile, of a conic-chain cylinder or a revolved conic.
    pub fn profile(&self) -> Option<&Profile> {
        match self {
            FormKey::ConicCylinder(k) => Some(&k.profile),
            FormKey::Revolved(k) => Some(&k.profile),
            _ => None,
        }
    }

    /// The kind's place in the order, its head values (module docs) and
    /// how many there are.
    fn head(&self) -> (u8, [f64; HEAD], usize) {
        let mut out = [0.0; HEAD];
        let mut n = 0;
        let mut put = |xs: &[f64]| {
            out[n..n + xs.len()].copy_from_slice(xs);
            n += xs.len();
        };
        let kind = match self {
            FormKey::Plane(k) => {
                put(&k.normal);
                put(&[k.offset]);
                0
            }
            FormKey::Cylinder(k) => {
                put(&k.axis);
                put(&k.foot);
                put(&[k.radius]);
                1
            }
            FormKey::ConicCylinder(k) => {
                put(&k.axis);
                2
            }
            FormKey::Cone(k) => {
                put(&k.apex);
                put(&k.axis);
                put(&[k.cos, k.sin]);
                3
            }
            FormKey::Sphere(k) => {
                put(&k.centre);
                put(&[k.radius]);
                put(&k.axis);
                4
            }
            FormKey::Torus(k) => {
                put(&k.centre);
                put(&k.axis);
                put(&[k.major, k.minor]);
                5
            }
            FormKey::Revolved(k) => {
                put(&k.axis);
                put(&k.foot);
                6
            }
            FormKey::Affine(k) => {
                put(k.linear.as_flattened());
                put(&k.shift);
                7
            }
        };
        (kind, out, n)
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

impl From<ConicCylinderKey> for FormKey {
    fn from(k: ConicCylinderKey) -> FormKey {
        FormKey::ConicCylinder(k)
    }
}

impl From<ConeKey> for FormKey {
    fn from(k: ConeKey) -> FormKey {
        FormKey::Cone(k)
    }
}

impl From<SphereKey> for FormKey {
    fn from(k: SphereKey) -> FormKey {
        FormKey::Sphere(k)
    }
}

impl From<TorusKey> for FormKey {
    fn from(k: TorusKey) -> FormKey {
        FormKey::Torus(k)
    }
}

impl From<RevolvedKey> for FormKey {
    fn from(k: RevolvedKey) -> FormKey {
        FormKey::Revolved(k)
    }
}

impl From<AffineKey> for FormKey {
    fn from(k: AffineKey) -> FormKey {
        FormKey::Affine(k)
    }
}

/// Keys order by kind (in the order the module docs list them), then by
/// their stored values in the order listed there, each by
/// `f64::total_cmp` (so `−0 < +0`); a profile by its number of spans,
/// then its values; an affine image's source last: a total order on the
/// bits.
impl Ord for FormKey {
    fn cmp(&self, o: &Self) -> Ordering {
        let (ka, a, n) = self.head();
        let (kb, b, _) = o.head();
        ka.cmp(&kb)
            .then_with(|| total_cmp(&a[..n], &b[..n]))
            .then_with(|| match (self.profile(), o.profile()) {
                (Some(p), Some(r)) => p
                    .spans
                    .len()
                    .cmp(&r.spans.len())
                    .then_with(|| total_cmp(p.values(), r.values())),
                _ => Ordering::Equal,
            })
            .then_with(|| match (self, o) {
                (FormKey::Affine(x), FormKey::Affine(y)) => x.source.cmp(&y.source),
                _ => Ordering::Equal,
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

/// `a` against `b` lexicographically, each value by `f64::total_cmp`, a
/// prefix first.
fn total_cmp(a: &[f64], b: &[f64]) -> Ordering {
    a.iter()
        .zip(b)
        .map(|(x, y)| x.total_cmp(y))
        .find(|c| c.is_ne())
        .unwrap_or_else(|| a.len().cmp(&b.len()))
}

/// A line's stored direction and foot point, with the exact direction
/// they were rounded from.
struct Axis {
    /// The stored unit direction (sign rule).
    stored: [f64; 3],
    /// The exact direction, a positive multiple of `stored`'s exact
    /// value.
    exact: [f64; 3],
    /// Whether `exact` points against the direction given.
    reversed: bool,
    /// The foot point, rounded.
    foot: [f64; 3],
}

impl Axis {
    fn new<C: Charge>(
        point: &[f64; 3],
        axis: &[f64; 3],
        m: &mut KeyMeter<'_, C>,
    ) -> Result<Axis, KeyError<C::Err>> {
        let (stored_dir, exact) = unit(axis, m)?;
        // `exact` is the direction given times a non-zero factor.
        let first = axis.iter().position(|x| *x != 0.0).unwrap_or(0);
        let reversed = (exact[first] < 0.0) != (axis[first] < 0.0);
        let p = point.map(q);
        let a = exact.map(q);
        let t = dot(&p, &a) / dot(&a, &a);
        let mut foot = [0.0; 3];
        for (k, f) in foot.iter_mut().enumerate() {
            *f = m
                .rational(|| &p[k] - &t * &a[k])
                .map_err(KeyError::Budget)?;
        }
        stored(&foot)?;
        Ok(Axis {
            stored: stored_dir,
            exact,
            reversed,
            foot,
        })
    }
}

/// Validates a chain of spans: control points `(point, w)` finite,
/// weights positive, consecutive spans sharing their end point.
fn chain(spans: impl Iterator<Item = [([f64; 3], f64); 3]>) -> Result<(), FormError> {
    let mut last: Option<[f64; 3]> = None;
    let mut any = false;
    for s in spans {
        any = true;
        for (p, w) in &s {
            finite(p)?;
            finite(&[*w])?;
            if *w <= 0.0 {
                return Err(FormError::Profile);
            }
        }
        if let Some(end) = last
            && end != s[0].0
        {
            return Err(FormError::Profile);
        }
        last = Some(s[2].0);
    }
    if any { Ok(()) } else { Err(FormError::Profile) }
}

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

/// `d` negated if its first non-zero component is negative, scaled by a
/// power of two where that is exact ([`scaled`]); whether it was negated.
fn signed_direction(d: &[f64; 3]) -> Result<([f64; 3], bool), FormError> {
    finite(d)?;
    let first = d
        .iter()
        .find(|x| **x != 0.0)
        .ok_or(FormError::ZeroDirection)?;
    let neg = *first < 0.0;
    let d = if neg { d.map(|x| -x) } else { *d };
    Ok((scaled(d), neg))
}

/// `d` times the power of two that brings its largest component into
/// `[1, 2)`, where every component scales exactly; else `d`. The keys'
/// formulas are invariant under positive scaling; this keeps their
/// first levels in range.
fn scaled(d: [f64; 3]) -> [f64; 3] {
    let big = d
        .iter()
        .fold(0.0f64, |m, x| if x.abs() > m { x.abs() } else { m });
    if big == 0.0 {
        return d;
    }
    let bits = big.to_bits();
    let field = ((bits >> 52) & 0x7ff) as i32;
    // `⌊log₂ big⌋`, also for a subnormal.
    let e = if field == 0 {
        (63 - bits.leading_zeros()) as i32 - 1074
    } else {
        field - 1023
    };
    let out = d.map(|x| times_pow2(x, -e));
    if out.iter().zip(&d).all(|(y, x)| times_pow2(*y, e) == *x) {
        out
    } else {
        d
    }
}

/// `x·2^k`, in steps that each scale by a normal power of two.
fn times_pow2(mut x: f64, mut k: i32) -> f64 {
    while k != 0 {
        let step = k.clamp(-1000, 1000);
        x *= f64::from_bits(((1023 + step) as u64) << 52);
        k -= step;
    }
    x
}

/// [`scaled`] for two values.
fn scaled2(d: [f64; 2]) -> [f64; 2] {
    let [a, b, _] = scaled([d[0], d[1], 0.0]);
    [a, b]
}

/// `a/|a|`, each component rounded.
fn components<C: Charge>(
    a: &[f64; 3],
    m: &mut KeyMeter<'_, C>,
) -> Result<[f64; 3], KeyError<C::Err>> {
    let e = a.map(Expr::Lit);
    let aa = Expr::dot(&e, &e);
    let mut u = [0.0; 3];
    for (k, c) in u.iter_mut().enumerate() {
        *c = m
            .irrational(&Quadratic::ratio(Expr::Lit(a[k]), aa.clone()))
            .map_err(KeyError::Budget)?;
    }
    Ok(u)
}

/// `d` normalised under the sign rule and rounded, with the exact
/// direction it was rounded from (negated with it where the rule
/// negates the rounded one).
fn unit<C: Charge>(d: &[f64; 3], m: &mut KeyMeter<'_, C>) -> Built<([f64; 3], [f64; 3]), C> {
    let (a, _) = signed_direction(d)?;
    let u = components(&a, m)?;
    stored(&u)?;
    let (u, flip) = sign_rule(u);
    Ok((u, if flip { a.map(|x| -x) } else { a }))
}

/// The world axis of least `|a_k|` (ties: the lowest `k`).
fn least(a: &[f64; 3]) -> usize {
    let mut k = 0;
    for c in 1..3 {
        if a[c].abs() < a[k].abs() {
            k = c;
        }
    }
    k
}

/// The rounded direction `u` with `+0` for zeros, negated if its first
/// non-zero component is negative (only where the exact direction's
/// leading components rounded to zero); whether it was negated.
fn sign_rule(u: [f64; 3]) -> ([f64; 3], bool) {
    let flip = u.iter().find(|x| **x != 0.0).is_some_and(|x| *x < 0.0);
    (u.map(|x| zero(if flip { -x } else { x })), flip)
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

//! Motions of space, and the moves and assemblies of solids by them.
//! [`Motion::align`] works out the motion that puts one body's point and
//! directions onto another's (see `transform/align.rs`).
//!
//! A [`Motion`] is an affine map `x ↦ L·x + t`, built as a move, a turn
//! about a line, a mirror in a plane or a scale about a point (uniform or
//! per world axis), or composed of them ([`Motion::then`]). Patterns place
//! copy `k` directly ([`Motion::pattern_step`], [`Motion::pattern_turn`]),
//! never by composing `k` steps, so no error piles up along them. Copies
//! are named by an [`Instance`]; a move keeps the names.
//!
//! [`Solid::transformed`] and [`assemble`] only handle the empty solid,
//! the only one the facade has, until the Varde kernel's motions land.
//!
//! Angles are in degrees, so that the quarter turns users type are exact:
//! a turn by a multiple of 90° takes its sine and cosine as `0` and `±1`,
//! not from [`trig`](crate::trig), and about a coordinate axis its matrix
//! has only those entries, so it maps coordinates to the bit. Other angles
//! go through [`trig`](crate::trig), the same bits on every platform.

use glam::{DMat3, DVec3};

use crate::mesh::FaceKey;
use crate::{Budget, Failure, KernelError, Solid, Tolerance};

mod align;

pub use align::{AlignError, AlignOptions, Datum, PARALLEL};

/// The largest factor [`Motion::scale`] takes, and the inverse of the
/// smallest: far beyond what a design asks (a scale's feature allows a
/// thousand), so a quadric's coefficients, which grow by the square of
/// the inverse, stay well within `f64`. The scaled points are bounded by
/// [`MAX_COORD`](crate::MAX_COORD) on their own.
pub const MAX_SCALE: f64 = 1e6;

/// An affine map of space, `x ↦ linear·x + offset`: a move, turn,
/// mirror or scale with positive factors, or a composition of them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    linear: DMat3,
    /// How normals and the coefficients of planes and quadrics map: the
    /// inverse transpose of `linear`, kept beside it rather than worked
    /// out, so a turn's is its own matrix to the bit.
    normal: DMat3,
    offset: DVec3,
    /// Whether it turns space inside out (`linear`'s determinant is
    /// negative): an odd number of mirrors.
    mirrors: bool,
    /// At least the most it stretches a length: 1 for a rigid motion, the
    /// largest factor of a scale, their product for a composition.
    stretch: f64,
    /// Whether it stretches every length by `stretch` (`linear` is
    /// `stretch` times an orthogonal matrix): rigid motions and uniform
    /// scales, and their compositions.
    uniform: bool,
}

/// Which copy a [`Solid::transformed`] makes: copy `index` that feature
/// `feature` (a pattern or a mirror) makes. Its faces are named by
/// [`FaceName::copy`](crate::mesh::FaceName::copy), so copies of copies stay unique and stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instance {
    pub feature: u64,
    pub index: u64,
}

impl Motion {
    /// The motion that moves nothing.
    pub const IDENTITY: Motion = Motion {
        linear: DMat3::IDENTITY,
        normal: DMat3::IDENTITY,
        offset: DVec3::ZERO,
        mirrors: false,
        stretch: 1.0,
        uniform: true,
    };

    /// The move by `offset`; `None` if it isn't finite.
    pub fn translation(offset: DVec3) -> Option<Motion> {
        offset.is_finite().then_some(Motion {
            offset,
            ..Motion::IDENTITY
        })
    }

    /// The turn by `degrees` about the line through `point` along `axis`
    /// (any length), counter-clockwise looking down `axis` (the
    /// right-hand rule). Multiples of 90° are exact (see the
    /// [module](self) docs); `None` for an angle or point that isn't
    /// finite, or a zero or non-finite axis.
    pub fn turn(point: DVec3, axis: DVec3, degrees: f64) -> Option<Motion> {
        if !point.is_finite() || !degrees.is_finite() {
            return None;
        }
        let k = unit(axis)?;
        let (sin, cos) = sin_cos_degrees(degrees);
        // Rodrigues: cos·I + sin·[k]× + (1 − cos)·k·kᵀ.
        let cross = DMat3::from_cols(
            DVec3::new(0.0, k.z, -k.y),
            DVec3::new(-k.z, 0.0, k.x),
            DVec3::new(k.y, -k.x, 0.0),
        );
        let linear = DMat3::IDENTITY * cos + cross * sin + outer(k, k) * (1.0 - cos);
        Some(Motion {
            linear,
            normal: linear,
            offset: point - linear * point,
            ..Motion::IDENTITY
        })
    }

    /// The mirror in the plane through `point` square to `normal`
    /// (`x ↦ x − 2·(n·(x − point))·n/|n|²`, `normal` of any length); exact
    /// in planes square to a coordinate axis. `None` for a point that
    /// isn't finite or a zero or non-finite normal.
    pub fn mirror(point: DVec3, normal: DVec3) -> Option<Motion> {
        let largest = normal.abs().max_element();
        if !point.is_finite() || !normal.is_finite() || largest == 0.0 {
            return None;
        }
        // Divided by its largest coordinate, so its square neither
        // overflows nor underflows, and a normal along an axis is that
        // axis exactly: its matrix's entries are then 0 and ±1, and the
        // offset twice the point's coordinate along it.
        let n = normal / largest;
        let nn = n.length_squared();
        // I − 2·n·nᵀ/|n|², symmetric and orthogonal: its own inverse
        // transpose.
        let linear = DMat3::IDENTITY - outer(n, n * (2.0 / nn));
        Some(Motion {
            linear,
            normal: linear,
            offset: n * (2.0 * n.dot(point) / nn),
            mirrors: true,
            ..Motion::IDENTITY
        })
    }

    /// The scale about `centre` by `factors` along the world axes, `x ↦
    /// centre + S·(x − centre)` with `S = diag(factors)`; uniform when the
    /// three are equal (`DVec3::splat(f)`). Each factor must lie within
    /// `1/MAX_SCALE ..= MAX_SCALE` (a negative one would be a mirror, its
    /// own motion). Exact to the bit for powers of two about a centre
    /// whose coordinates they scale exactly (the origin, say). `None` for
    /// a factor out of range or NaN, or a centre that isn't finite or
    /// would put the offset past `f64`'s range.
    pub fn scale(centre: DVec3, factors: DVec3) -> Option<Motion> {
        let range = 1.0 / MAX_SCALE..=MAX_SCALE;
        if !factors.to_array().iter().all(|f| range.contains(f)) || !centre.is_finite() {
            return None;
        }
        let linear = DMat3::from_diagonal(factors);
        let offset = centre - factors * centre;
        offset.is_finite().then_some(Motion {
            linear,
            normal: DMat3::from_diagonal(factors.recip()),
            offset,
            mirrors: false,
            stretch: factors.max_element(),
            uniform: factors.x == factors.y && factors.y == factors.z,
        })
    }

    /// Copy `k` of a linear pattern: the move by `k·spacing` along
    /// `direction` (any length), placed directly. `None` for a zero or
    /// non-finite direction, or an offset that isn't finite.
    pub fn pattern_step(direction: DVec3, spacing: f64, k: u32) -> Option<Motion> {
        let d = unit(direction)?;
        Motion::translation(d * (f64::from(k) * spacing))
    }

    /// Copy `k` of a circular pattern of `count` copies spread evenly over
    /// `degrees` (360 for a full turn): the turn by `k·degrees/count`
    /// about the line through `point` along `axis`, placed directly.
    /// `None` for no copies, or as [`Motion::turn`].
    pub fn pattern_turn(
        point: DVec3,
        axis: DVec3,
        degrees: f64,
        k: u32,
        count: u32,
    ) -> Option<Motion> {
        if count == 0 {
            return None;
        }
        Motion::turn(point, axis, f64::from(k) * degrees / f64::from(count))
    }

    /// `self`, then `next`.
    pub fn then(&self, next: &Motion) -> Motion {
        Motion {
            linear: next.linear * self.linear,
            normal: next.normal * self.normal,
            offset: next.linear * self.offset + next.offset,
            mirrors: self.mirrors != next.mirrors,
            stretch: self.stretch * next.stretch,
            uniform: self.uniform && next.uniform,
        }
    }

    /// Where it takes the point `p`.
    pub fn point(&self, p: DVec3) -> DVec3 {
        self.linear * p + self.offset
    }

    /// Where it takes the direction `v` (a difference of points).
    pub fn vector(&self, v: DVec3) -> DVec3 {
        self.linear * v
    }

    /// Where it takes the normal `n` of a surface: square to the mapped
    /// surface, pointing to the image of the side `n` pointed to. Not
    /// normalized.
    pub fn normal(&self, n: DVec3) -> DVec3 {
        self.normal * n
    }

    /// Whether it turns space inside out: a mirror.
    pub fn mirrors(&self) -> bool {
        self.mirrors
    }

    /// At least the most it stretches a length: 1 for rigid motions, a
    /// scale's largest factor, the product of those it is composed of.
    pub fn stretch(&self) -> f64 {
        self.stretch
    }

    /// Its numbers' bits, for keying what it makes: `linear`'s, the map of
    /// normals' and the offset's (columns in order), then whether it
    /// mirrors, its stretch and whether it's uniform. Motions with the
    /// same bits move every solid alike, to the bit.
    pub fn bits(&self) -> [u64; 24] {
        let mut bits = [0; 24];
        let numbers = (self.linear.to_cols_array().into_iter())
            .chain(self.normal.to_cols_array())
            .chain(self.offset.to_array())
            .chain([self.stretch]);
        for (slot, number) in bits.iter_mut().zip(numbers) {
            *slot = number.to_bits();
        }
        bits[22] = u64::from(self.mirrors);
        bits[23] = u64::from(self.uniform);
        bits
    }
}

/// The sine and cosine of `degrees`: exactly `0` and `±1` at multiples of
/// 90°, else through [`trig`](crate::trig) after reducing the angle to
/// `[−180°, 180°]` (the reductions by 360° and 90° are exact: `%` on
/// floats is).
fn sin_cos_degrees(degrees: f64) -> (f64, f64) {
    let mut r = degrees % 360.0;
    if r > 180.0 {
        r -= 360.0;
    } else if r < -180.0 {
        r += 360.0;
    }
    if r % 90.0 == 0.0 {
        // r is one of −180, −90, 0, 90, 180.
        return match r as i32 {
            90 => (1.0, 0.0),
            -90 => (-1.0, 0.0),
            180 | -180 => (0.0, -1.0),
            _ => (0.0, 1.0),
        };
    }
    crate::trig::sin_cos(r * (std::f64::consts::PI / 180.0))
}

/// `v` scaled to unit length by dividing by its length, after dividing by
/// its largest coordinate (so its square neither overflows nor
/// underflows, and a vector along a world axis is that axis exactly);
/// `None` for a zero or non-finite vector.
pub(crate) fn unit(v: DVec3) -> Option<DVec3> {
    let largest = v.abs().max_element();
    if !v.is_finite() || largest == 0.0 {
        return None;
    }
    let v = v / largest;
    Some(v / v.length())
}

/// `u·vᵀ`.
fn outer(u: DVec3, v: DVec3) -> DMat3 {
    DMat3::from_cols(u * v.x, u * v.y, u * v.z)
}

impl FaceKey {
    /// The key of copy `index` that `feature` makes of a face so keyed:
    /// what [`FaceName::copy`](crate::mesh::FaceName::copy) makes of its names' keys.
    pub fn copy(self, feature: u64, index: u64) -> FaceKey {
        FaceKey {
            instance: crate::topology::mix(&[self.instance, feature, index]),
            ..self
        }
    }
}

impl Solid {
    /// The solid moved by `motion`, its faces renamed as copy `copy` (see
    /// [`Instance`]) or, with `None`, keeping their names (a move).
    ///
    /// The empty solid moves to itself; any other is
    /// [`KernelError::NotImplemented`] until the Varde kernel's motions
    /// land (no other solid exists meanwhile).
    pub fn transformed(
        &self,
        _motion: &Motion,
        _copy: Option<Instance>,
        _tol: &Tolerance,
        _budget: &Budget,
    ) -> Result<Solid, Failure> {
        if self.is_empty() {
            Ok(Solid::empty())
        } else {
            Err(KernelError::NotImplemented("moving a solid").into())
        }
    }
}

/// One solid of several, such as a pattern's copies. No parts, or only
/// empty ones, give the empty solid; any other is
/// [`KernelError::NotImplemented`] until the Varde kernel's assembly
/// lands.
pub fn assemble(parts: &[Solid], _tol: &Tolerance, _budget: &Budget) -> Result<Solid, Failure> {
    if parts.iter().all(Solid::is_empty) {
        Ok(Solid::empty())
    } else {
        Err(KernelError::NotImplemented("assembling solids").into())
    }
}

#[cfg(test)]
mod tests;

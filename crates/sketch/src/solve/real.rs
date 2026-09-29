//! The numbers equations are written over: `f64` for their values, and
//! forward-mode dual numbers for their derivatives.

use std::ops::{Add, Div, Mul, Neg, Sub};

/// The most variables one equation reads, the length of a [`Dual`]'s
/// gradient: sixteen at most (an equal offset's two pairs of lines). An
/// equation reading a spline, whose points may be hundreds, reads it as
/// two numbers per derivative, ten at most with its own (a smooth join of
/// a spline and an arc), and is differentiated through the spline's map
/// by the chain rule (see `Residual::gradient`), so it stays sixteen.
pub(crate) const MAX_INPUTS: usize = 16;

/// What an equation is computed with, so it's written once and evaluated
/// both for its value and for its derivatives.
pub(crate) trait Real:
    Copy
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
    + Neg<Output = Self>
{
    fn constant(value: f64) -> Self;
    fn sqrt(self) -> Self;
}

impl Real for f64 {
    fn constant(value: f64) -> Self {
        value
    }

    fn sqrt(self) -> Self {
        f64::sqrt(self)
    }
}

/// A value with its derivatives by up to [`MAX_INPUTS`] variables.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Dual {
    pub value: f64,
    pub gradient: [f64; MAX_INPUTS],
}

impl Dual {
    /// The `index`th variable, at `value`.
    pub fn variable(value: f64, index: usize) -> Self {
        let mut gradient = [0.0; MAX_INPUTS];
        gradient[index] = 1.0;
        Dual { value, gradient }
    }

    fn map(self, value: f64, scale: f64) -> Self {
        Dual {
            value,
            gradient: self.gradient.map(|d| d * scale),
        }
    }

    fn zip(self, other: Self, value: f64, f: impl Fn(f64, f64) -> f64) -> Self {
        let mut gradient = self.gradient;
        for (d, o) in gradient.iter_mut().zip(other.gradient) {
            *d = f(*d, o);
        }
        Dual { value, gradient }
    }
}

impl Real for Dual {
    fn constant(value: f64) -> Self {
        Dual {
            value,
            gradient: [0.0; MAX_INPUTS],
        }
    }

    /// At zero, where the slope is infinite, the derivatives are taken
    /// as zero: a length of zero (two points at one place) has no
    /// direction to grow in, and an infinite derivative would make the
    /// whole step not a number.
    fn sqrt(self) -> Self {
        let value = self.value.sqrt();
        let scale = if value == 0.0 { 0.0 } else { 0.5 / value };
        self.map(value, scale)
    }
}

impl Add for Dual {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        self.zip(other, self.value + other.value, |a, b| a + b)
    }
}

impl Sub for Dual {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        self.zip(other, self.value - other.value, |a, b| a - b)
    }
}

impl Mul for Dual {
    type Output = Self;
    #[expect(clippy::suspicious_arithmetic_impl, reason = "the product rule")]
    fn mul(self, other: Self) -> Self {
        let (a, b) = (self.value, other.value);
        self.zip(other, a * b, |da, db| da * b + a * db)
    }
}

impl Div for Dual {
    type Output = Self;
    #[expect(clippy::suspicious_arithmetic_impl, reason = "the quotient rule")]
    fn div(self, other: Self) -> Self {
        let value = self.value / other.value;
        let b = other.value;
        self.zip(other, value, |da, db| (da - value * db) / b)
    }
}

impl Neg for Dual {
    type Output = Self;
    fn neg(self) -> Self {
        self.map(-self.value, -1.0)
    }
}

/// A 2D vector of [`Real`]s, for writing equations as geometry.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Vector<R> {
    pub x: R,
    pub y: R,
}

impl<R: Real> Vector<R> {
    pub fn dot(self, other: Self) -> R {
        self.x * other.x + self.y * other.y
    }

    /// The z of the 3D cross product: positive when `other` is
    /// counter-clockwise of `self`.
    pub fn cross(self, other: Self) -> R {
        self.x * other.y - self.y * other.x
    }

    pub fn length(self) -> R {
        self.dot(self).sqrt()
    }

    /// Halfway between `self` and `other`.
    pub fn midpoint(self, other: Self) -> Self {
        let half = R::constant(0.5);
        Vector {
            x: (self.x + other.x) * half,
            y: (self.y + other.y) * half,
        }
    }
}

impl<R: Real> Sub for Vector<R> {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Vector {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}
